//! Bias the screen-space decal permutations' depth fetch into one eye's half of a double-wide target.
//!
//! The `ssdecal*` pixel shaders divide their interpolated projective screen coordinate by `w` and then
//! use the resulting UV for **two** different things:
//!
//! ```text
//! div r0.xy, v1.xyxx, v1.wwww          // uv, normalized over the viewport
//! mul r1.xyzw, r0.yyyy, cb1[1].xyzw    // uv through the pass's reconstruction basis ...
//! mad r1.xyzw, r0.xxxx, cb1[0].xyzw, r1.xyzw
//! mul r0.xy, r0.xyxx, cb1[12].xyxx     // ... and the scene-depth fetch UV
//! sample_indexable(texture2d) r0.x, r0.xyxx, t0.xyzw, s0
//! ```
//!
//! The basis rows want the *viewport-normalized* value; the depth fetch wants a UV over the whole
//! bound depth texture. Those coincide when the viewport covers the texture and differ when it does
//! not, and no scale applied to `cb1[12]` can reconcile them, because the two differ by an offset as
//! well as a scale.
//!
//! [`bias_ssdecal_depth_uv`] splits them by inserting one instruction between the last basis row and
//! the depth-UV multiply:
//!
//! ```text
//! mad r0.x, r0.x, l(0.5), cb1[13].x
//! ```
//!
//! and widening the `dcl_constantbuffer cb1[13]` declaration to `cb1[14]` so the new register is in
//! range. `cb1[13].x` is a caller-staged horizontal offset; with it at `0.0` the fetch addresses the
//! left half of the texture and at `0.5` the right half. The reconstruction rows are upstream of the
//! insertion and keep the untouched per-viewport value.
//!
//! The site is matched structurally, not by name: a multiply of a temp by `cb1[12].xyxx` whose result
//! is immediately sampled from `t0`, with that temp produced by an earlier `div`. In the game's
//! bundle that matches exactly the twelve `ssdecal*` fragment permutations.

use crate::{
    container::{Dxbc, DxbcError},
    tokens::{ShaderStage, TokenStream, parse_operand},
};

use super::common::{OPERAND_IMM32_SCALAR, OPERAND_TEMP_MASK_X, OPERAND_TEMP_SELECT_X};

/// The fragment `cb1` register the inserted `mad` reads its horizontal offset from. The `ssdecal*`
/// permutations declare `cb1[13]`, so register 13 is the first one past what they already use, and the
/// rewrite widens the declaration to cover it.
pub const SSDECAL_EYE_BIAS_REGISTER: u32 = 13;

/// Insert the depth-UV bias into an `ssdecal*` pixel shader, returning the rewritten container.
///
/// Returns [`DxbcError::NoDepthUvFetch`] for any shader that does not carry the site (every shader
/// that is not one of the decal permutations), which the caller treats as "leave this one alone".
pub fn bias_ssdecal_depth_uv(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let shex = dxbc.shader_chunk().ok_or(DxbcError::NoShaderChunk)?;
    let stream = TokenStream::new(shex.body(blob))?;
    if stream.stage() != ShaderStage::Pixel {
        return Err(DxbcError::NoDepthUvFetch);
    }
    let tokens = stream.tokens();

    let site = find_site(&stream)?;
    if site.cb1_size_token >= site.insert_at {
        // The declaration must precede the code for the widened size to be in force at the insertion,
        // and for the token index to survive the splice unshifted.
        return Err(DxbcError::NoDepthUvFetch);
    }

    let mut out = Vec::with_capacity(tokens.len() + BIAS_INSTRUCTION_DWORDS);
    out.extend_from_slice(&tokens[..site.insert_at]);
    out.extend_from_slice(&bias_instruction(site.temp));
    out.extend_from_slice(&tokens[site.insert_at..]);
    out[site.cb1_size_token] = out[site.cb1_size_token].max(SSDECAL_EYE_BIAS_REGISTER + 1);
    // The token array carries the chunk header, so the length dword has to grow with the splice.
    out[1] = out.len() as u32;

    let mut new_shex = Vec::with_capacity(out.len() * 4);
    for token in &out {
        new_shex.extend_from_slice(&token.to_le_bytes());
    }
    let chunk_body = |tag: &[u8; 4]| {
        dxbc.chunk(tag)
            .map(|chunk| chunk.body(blob).to_vec())
            .unwrap_or_default()
    };
    // No signature or feature-flag change: the rewrite adds arithmetic and a constant-buffer row, so
    // the two signature chunks are handed back exactly as they came in.
    Ok(super::common::reassemble(
        &dxbc,
        blob,
        chunk_body(b"ISGN"),
        chunk_body(b"OSGN"),
        new_shex,
        false,
    ))
}

/// The dwords of the inserted `mad`: the opcode, a two-dword destination and two-dword temp source, a
/// two-dword scalar immediate, and a three-dword `cbN[r]` source.
const BIAS_INSTRUCTION_DWORDS: usize = 10;

/// The horizontal scale the bias applies: the whole viewport-normalized UV maps onto half of the bound
/// texture, and `cb1[13].x` chooses which half.
const HALF: f32 = 0.5;

/// `mad r<temp>.x, r<temp>.x, l(0.5), cb1[13].x`.
fn bias_instruction(temp: u32) -> [u32; BIAS_INSTRUCTION_DWORDS] {
    [
        ((BIAS_INSTRUCTION_DWORDS as u32) << 24) | OPCODE_MAD,
        OPERAND_TEMP_MASK_X,
        temp,
        OPERAND_TEMP_SELECT_X,
        temp,
        OPERAND_IMM32_SCALAR,
        HALF.to_bits(),
        OPERAND_CB_2D_IMM_SELECT_X,
        DEPTH_UV_CB,
        SSDECAL_EYE_BIAS_REGISTER,
    ]
}

/// Where the rewrite acts on one shader.
struct Site {
    /// The token index of the `mul` that forms the depth-fetch UV; the bias goes immediately before it.
    insert_at: usize,
    /// The temp register the UV lives in, whose `.x` the bias rewrites.
    temp: u32,
    /// The token index of the `dcl_constantbuffer cb1[N]` size dword, widened to cover the new register.
    cb1_size_token: usize,
}

/// Locate the depth-fetch site: a `mul` of a temp by `cb1[12].xyxx` whose result the next instruction
/// samples from `t0`, with that temp written by an earlier `div`.
///
/// All three conditions are load-bearing. The `div` is what makes the temp a *projective* screen UV
/// rather than an arbitrary pair; the `cb1[12]` multiply is the depth-texture scale; and the `t0`
/// sample is what says the UV addresses the scene depth buffer rather than a material texture.
fn find_site(stream: &TokenStream) -> Result<Site, DxbcError> {
    let tokens = stream.tokens();
    let instructions = stream
        .instructions()
        .map(|insn| {
            insn.map(|insn| Insn {
                opcode: insn.opcode,
                operands_start: insn.operands_start(),
                start: insn.start,
                end: insn.end,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut cb1_size_token = None;
    let mut divided: Vec<u32> = Vec::new();
    for (index, insn) in instructions.iter().enumerate() {
        if insn.opcode == OPCODE_DCL_CONSTANT_BUFFER {
            if let Some(at) = constant_buffer_size_token(tokens, insn) {
                cb1_size_token = Some(at);
            }
            continue;
        }
        let operands = decode_operands(tokens, insn)?;
        if insn.opcode == OPCODE_DIV {
            if let Some(dest) = operands.first().filter(|o| o.ty == OPERAND_TYPE_TEMP) {
                divided.extend(dest.index0);
            }
            continue;
        }
        if insn.opcode != OPCODE_MUL {
            continue;
        }
        let [dest, source, scale] = operands.as_slice() else {
            continue;
        };
        let (Some(temp), Some(from)) = (dest.index0, source.index0) else {
            continue;
        };
        let is_depth_uv = dest.ty == OPERAND_TYPE_TEMP
            && source.ty == OPERAND_TYPE_TEMP
            && temp == from
            && divided.contains(&temp)
            && scale.ty == OPERAND_TYPE_CONSTANT_BUFFER
            && scale.index0 == Some(DEPTH_UV_CB)
            && scale.index1 == Some(DEPTH_UV_SCALE_REGISTER);
        if !is_depth_uv {
            continue;
        }
        let Some(next) = instructions.get(index + 1) else {
            continue;
        };
        if next.opcode != OPCODE_SAMPLE || !samples_scene_depth(tokens, next, temp)? {
            continue;
        }
        let cb1_size_token = cb1_size_token.ok_or(DxbcError::NoDepthUvFetch)?;
        return Ok(Site {
            insert_at: insn.start,
            temp,
            cb1_size_token,
        });
    }
    Err(DxbcError::NoDepthUvFetch)
}

/// Whether a `sample` reads texture `t0` through the given temp as its address.
fn samples_scene_depth(tokens: &[u32], insn: &Insn, address: u32) -> Result<bool, DxbcError> {
    let operands = decode_operands(tokens, insn)?;
    // `sample dest, address, resource, sampler`.
    let [_, source, resource, _] = operands.as_slice() else {
        return Ok(false);
    };
    Ok(source.ty == OPERAND_TYPE_TEMP
        && source.index0 == Some(address)
        && resource.ty == OPERAND_TYPE_RESOURCE
        && resource.index0 == Some(SCENE_DEPTH_TEXTURE))
}

/// The token index of a `dcl_constantbuffer cb<DEPTH_UV_CB>[N]`'s size dword, or `None` for any other
/// declaration. The declaration's operand is `cb<register>[<size>]` with both indices immediate, so
/// the size is the dword after the register.
fn constant_buffer_size_token(tokens: &[u32], insn: &Insn) -> Option<usize> {
    let token = *tokens.get(insn.start + 1)?;
    let extended = ((token >> 31) & 1) as usize;
    let register_at = insn.start + 2 + extended;
    let size_at = register_at + 1;
    (size_at < insn.end && *tokens.get(register_at)? == DEPTH_UV_CB).then_some(size_at)
}

/// One instruction, reduced to what the matcher needs.
struct Insn {
    opcode: u32,
    operands_start: usize,
    start: usize,
    end: usize,
}

/// One decoded operand: its type and its immediate indices, which is all the matcher discriminates on.
struct DecodedOperand {
    ty: u32,
    index0: Option<u32>,
    index1: Option<u32>,
}

fn decode_operands(tokens: &[u32], insn: &Insn) -> Result<Vec<DecodedOperand>, DxbcError> {
    let mut operands = Vec::new();
    let mut pos = insn.operands_start;
    while pos < insn.end {
        let (_, next) = parse_operand(tokens, pos)?;
        if next <= pos {
            return Err(DxbcError::UnexpectedEndOfTokens);
        }
        let token = tokens[pos];
        let dimensions = (token >> 20) & 0x3;
        let first_index = pos + 1 + ((token >> 31) & 1) as usize;
        let mut index = [None, None];
        for (d, slot) in index
            .iter_mut()
            .enumerate()
            .take(dimensions.min(2) as usize)
        {
            // Only immediate indices are decoded; a relative one leaves the rest `None`, which no
            // match arm accepts.
            if (token >> (22 + 3 * d)) & 0x7 != 0 {
                break;
            }
            *slot = tokens.get(first_index + d).copied();
        }
        operands.push(DecodedOperand {
            ty: (token >> 12) & 0xFF,
            index0: index[0],
            index1: index[1],
        });
        pos = next;
    }
    Ok(operands)
}

// SM4/5 opcodes the matcher and the injected instruction use.
const OPCODE_DIV: u32 = 0x0E;
const OPCODE_MAD: u32 = 0x32;
const OPCODE_MUL: u32 = 0x38;
const OPCODE_SAMPLE: u32 = 0x45;
const OPCODE_DCL_CONSTANT_BUFFER: u32 = 0x59;

// SM4/5 operand types.
const OPERAND_TYPE_TEMP: u32 = 0;
const OPERAND_TYPE_RESOURCE: u32 = 7;
const OPERAND_TYPE_CONSTANT_BUFFER: u32 = 8;

/// `cbN[r].x` as a source: constant buffer, 2D immediate indices, select-1 component `.x` -- the
/// select-1 form of `OPERAND_CB_2D_IMM`.
const OPERAND_CB_2D_IMM_SELECT_X: u32 = 0x0020_800A;

/// The fragment constant-buffer slot the decal permutations read: their instance constants, and the
/// slot the rewrite's new register is appended to.
const DEPTH_UV_CB: u32 = 1;

/// The register within [`DEPTH_UV_CB`] holding the depth texture's UV scale, which the depth-fetch
/// multiply is recognised by.
const DEPTH_UV_SCALE_REGISTER: u32 = 12;

/// The texture slot the decal permutations sample the scene depth from.
const SCENE_DEPTH_TEXTURE: u32 = 0;

#[cfg(test)]
mod tests {
    use super::*;

    /// The injected `mad` must decode as exactly one well-formed instruction of the declared length,
    /// with the operand shapes the rewrite intends -- a token-level typo here would be a shader that
    /// fails to create at runtime, far from this crate.
    #[test]
    fn the_bias_instruction_is_well_formed() {
        let mad = bias_instruction(7);
        assert_eq!(mad[0] & 0x7FF, OPCODE_MAD);
        assert_eq!(((mad[0] >> 24) & 0x7F) as usize, BIAS_INSTRUCTION_DWORDS);
        assert_eq!(mad.len(), BIAS_INSTRUCTION_DWORDS);

        let insn = Insn {
            opcode: OPCODE_MAD,
            operands_start: 1,
            start: 0,
            end: mad.len(),
        };
        let operands = decode_operands(&mad, &insn).expect("the injected mad decodes");
        assert_eq!(operands.len(), 4, "dest, temp, immediate, constant");
        assert_eq!(operands[0].ty, OPERAND_TYPE_TEMP);
        assert_eq!(operands[0].index0, Some(7));
        assert_eq!(operands[1].ty, OPERAND_TYPE_TEMP);
        assert_eq!(operands[1].index0, Some(7));
        assert_eq!(operands[3].ty, OPERAND_TYPE_CONSTANT_BUFFER);
        assert_eq!(operands[3].index0, Some(DEPTH_UV_CB));
        assert_eq!(operands[3].index1, Some(SSDECAL_EYE_BIAS_REGISTER));
    }

    /// A shader with no depth-fetch site must be declined rather than mangled -- the rewrite is offered
    /// every fragment shader the engine creates.
    #[test]
    fn a_shader_without_the_site_is_declined() {
        assert_eq!(
            bias_ssdecal_depth_uv(&[]).unwrap_err(),
            DxbcError::NotDxbc,
            "a non-container is rejected before any token walk",
        );
    }
}
