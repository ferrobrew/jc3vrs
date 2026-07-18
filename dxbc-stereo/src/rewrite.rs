//! The single-pass stereo vertex-shader rewrite.
//!
//! Transforms an opaque model vertex shader so one instance-doubled draw renders both eyes: a
//! mod-owned `cb13` carries both eyes' five per-eye rows (`[eye0: rows 0..4][eye1: rows 5..9]`,
//! view-projection rows first, the camera position last), and the shader picks its eye's block by
//! `SV_InstanceID & 1`, routing the result with `SV_ViewportArrayIndex`. Concretely:
//!
//! 1. Declare `cb13[10]` dynamicIndexed, an `SV_InstanceID` input at the next free `v` register, an
//!    `SV_ViewportArrayIndex` output at the next free `o` register, and one extra temp.
//! 2. Prologue (before the first real instruction): `and rBase.x, vN.x, l(1)` computes the eye,
//!    `mov oM.x, rBase.x` routes the viewport, then `imul null, rBase.x, rBase.x, l(5)` scales the
//!    eye to its `cb13` row base -- the viewport write happens first, so overwriting the temp with
//!    the scaled value is safe.
//! 3. Remap every `cb0[{4,29..32}]` operand to the register-relative `cb13[rBase.x + k]` (`k = 4`
//!    for the camera row, `n - 29` for the view-projection rows).
//! 4. Extend `ISGN`/`OSGN` with the new elements, and set bit 13 of `SFI0`
//!    (`VPAndRTArrayIndexFromAnyShaderFeedingRasterizer`) so the viewport output is legal from a
//!    vertex shader.
//!
//! The token and signature encodings reproduce byte-for-byte what fxc emits for a vs_5_0 that does
//! the same thing (the committed `tests/data/ref_vs50.dxbc`); see `docs/mod/single-pass-stereo.md`.

use crate::{
    PER_EYE_CB0_ROWS,
    checksum::refresh_checksum,
    container::{Dxbc, DxbcError},
    tokens::{
        IDX_IMM32_PLUS_RELATIVE, Instruction, OperandKind, ShaderStage, TokenStream, parse_operand,
    },
};

/// The constant-buffer slot the stereo constants bind to. `b13` is free across the game's vertex
/// shaders (they use `cb0`, `cb2`, and `cb12`).
pub const STEREO_CB_REGISTER: u32 = 13;
/// The stereo constant buffer's size in float4 rows: five per eye (four view-projection rows, then
/// the camera position), two eyes.
pub const STEREO_CB_ROWS: u32 = 10;

/// Rewrites a model vertex shader for single-pass stereo (see the module docs) and returns a new,
/// checksum-valid DXBC container. Fails without side effects if the shader is not a `SHEX` vertex
/// shader of the expected shape (no per-eye `cb0` rows, `cb13` or `SV_InstanceID` already taken,
/// missing signatures).
pub fn patch_vertex_shader(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    if dxbc.chunk(b"SHEX").is_none() && dxbc.chunk(b"SHDR").is_some() {
        return Err(DxbcError::UnsupportedShaderModel);
    }
    let shex = dxbc.chunk(b"SHEX").ok_or(DxbcError::NoShaderChunk)?;
    let isgn = dxbc
        .chunk(b"ISGN")
        .ok_or(DxbcError::MissingInputSignature)?;
    let osgn = dxbc
        .chunk(b"OSGN")
        .ok_or(DxbcError::MissingOutputSignature)?;

    let stream = TokenStream::new(shex.body(blob))?;
    if stream.stage() != ShaderStage::Vertex {
        return Err(DxbcError::NotVertexShader);
    }

    let plan = plan_rewrite(&stream)?;
    let new_shex = rewrite_shader(&stream, &plan)?;

    let new_isgn = append_signature_element(
        isgn.body(blob),
        &SignatureElement {
            name: "SV_InstanceID",
            semantic_index: 0,
            system_value: SIGNATURE_SYSVALUE_INSTANCE_ID,
            component_type: SIGNATURE_COMPONENT_UINT32,
            register: plan.input_register,
            mask: 0x01,
            // For inputs the second mask is the read mask: the prologue reads `.x`.
            rw_mask: 0x01,
        },
    )?;
    let new_osgn = append_signature_element(
        osgn.body(blob),
        &SignatureElement {
            name: "SV_ViewportArrayIndex",
            semantic_index: 0,
            system_value: SIGNATURE_SYSVALUE_VIEWPORT_ARRAY_INDEX,
            component_type: SIGNATURE_COMPONENT_UINT32,
            register: plan.output_register,
            mask: 0x01,
            // For outputs the second mask is the never-written mask: only `.x` is written.
            rw_mask: 0x0E,
        },
    )?;

    // Reassemble the container, swapping in the rewritten chunks and adding `SFI0` after the shader
    // chunk if the container had none (fxc's chunk order is RDEF, ISGN, OSGN, SHEX, SFI0, STAT).
    let mut chunks: Vec<([u8; 4], Vec<u8>)> = Vec::with_capacity(dxbc.chunks().len() + 1);
    let has_sfi0 = dxbc.chunk(b"SFI0").is_some();
    for chunk in dxbc.chunks() {
        let body = match &chunk.tag {
            b"ISGN" => new_isgn.clone(),
            b"OSGN" => new_osgn.clone(),
            b"SHEX" => new_shex.clone(),
            b"SFI0" => with_viewport_feature_bit(chunk.body(blob)),
            _ => chunk.body(blob).to_vec(),
        };
        chunks.push((chunk.tag, body));
        if &chunk.tag == b"SHEX" && !has_sfi0 {
            chunks.push((*b"SFI0", with_viewport_feature_bit(&[])));
        }
    }

    let mut out = build_container(&chunks);
    refresh_checksum(&mut out);
    Ok(out)
}

/// What the declaration scan learned: where to inject, and which registers are free.
struct RewritePlan {
    /// The `v` register for the new `SV_InstanceID` input (max declared input register + 1).
    input_register: u32,
    /// The `o` register for the new `SV_ViewportArrayIndex` output (max declared output + 1).
    output_register: u32,
    /// The temp holding the eye and then the eye's `cb13` row base (the old `dcl_temps` count).
    temp_register: u32,
    /// The token index of the existing `dcl_temps` instruction, if any (its count is bumped).
    dcl_temps_start: Option<usize>,
    /// The token index of the first non-declaration instruction -- the injection point for the new
    /// declarations and the prologue.
    inject_before: usize,
}

// SM4/5 opcodes.
const OPCODE_AND: u32 = 0x01;
const OPCODE_IMUL: u32 = 0x26;
const OPCODE_MOV: u32 = 0x36;
const OPCODE_DCL_CONSTANT_BUFFER: u32 = 0x59;
const OPCODE_DCL_INPUT: u32 = 0x5F;
const OPCODE_DCL_INPUT_SGV: u32 = 0x60;
const OPCODE_DCL_INPUT_SIV: u32 = 0x61;
const OPCODE_DCL_OUTPUT: u32 = 0x65;
const OPCODE_DCL_OUTPUT_SGV: u32 = 0x66;
const OPCODE_DCL_OUTPUT_SIV: u32 = 0x67;
const OPCODE_DCL_TEMPS: u32 = 0x68;

/// The `dynamicIndexed` access pattern, bit 11 of a `dcl_constantbuffer` opcode token. Required
/// because `cb13` is indexed through a register, unlike the game's `immediateIndexed` `cb0`.
const CB_ACCESS_DYNAMIC_INDEXED: u32 = 1 << 11;

// SM4/5 system-value names, as they appear in `dcl_*_sgv`/`dcl_*_siv` trailing tokens.
const SB_NAME_VIEWPORT_ARRAY_INDEX: u32 = 5;
const SB_NAME_INSTANCE_ID: u32 = 8;

// Signature (`ISGN`/`OSGN`) field values.
const SIGNATURE_SYSVALUE_VIEWPORT_ARRAY_INDEX: u32 = 5;
const SIGNATURE_SYSVALUE_INSTANCE_ID: u32 = 8;
const SIGNATURE_COMPONENT_UINT32: u32 = 1;

// Operand tokens for the injected code, byte-identical to what fxc emits (see the module docs).
/// `cbN[a][b]`: constant buffer, 2D immediate indices, 4-component swizzle `.xyzw`.
const OPERAND_CB_2D_IMM: u32 = 0x0020_8E46;
/// `vN.x` as a source: input register, 1D immediate index, select-1 component `.x`.
const OPERAND_INPUT_SELECT_X: u32 = 0x0010_100A;
/// `vN.x` in a declaration: input register, 1D immediate index, write-mask-form `.x` (declarations
/// use the mask form, not a swizzle).
const OPERAND_INPUT_MASK_X: u32 = 0x0010_1012;
/// `oM.x` as a destination: output register, 1D immediate index, write mask `.x`.
const OPERAND_OUTPUT_MASK_X: u32 = 0x0010_2012;
/// `rN.x` as a destination: temp register, 1D immediate index, write mask `.x`.
const OPERAND_TEMP_MASK_X: u32 = 0x0010_0012;
/// `rN.x` as a source (and as a relative index): temp register, 1D immediate index, select-1 `.x`.
const OPERAND_TEMP_SELECT_X: u32 = 0x0010_000A;
/// A scalar 32-bit immediate (`l(k)`).
const OPERAND_IMM32_SCALAR: u32 = 0x0000_4001;
/// The null destination (`imul`'s unused high-half result).
const OPERAND_NULL: u32 = 0x0000_D000;

/// `D3D_SHADER_REQUIRES_VIEWPORT_AND_RT_ARRAY_INDEX_FROM_ANY_SHADER_FEEDING_RASTERIZER`, bit 13 of
/// the `SFI0` feature flags. Without it the viewport output is invalid from a vertex shader.
const SFI0_VIEWPORT_FROM_ANY_STAGE: u64 = 1 << 13;

/// Scans the declarations: the next free input/output registers, the temp count, the injection
/// point, and the preconditions (`cb13` and `SV_InstanceID` unclaimed, per-eye rows present).
fn plan_rewrite(stream: &TokenStream) -> Result<RewritePlan, DxbcError> {
    let tokens = stream.tokens();
    let mut max_input: Option<u32> = None;
    let mut max_output: Option<u32> = None;
    let mut temps: u32 = 0;
    let mut dcl_temps_start = None;
    let mut inject_before = None;
    let mut per_eye_found = false;

    for insn in stream.instructions() {
        let insn = insn?;
        if insn.is_declaration() {
            match insn.opcode {
                OPCODE_DCL_INPUT | OPCODE_DCL_INPUT_SGV | OPCODE_DCL_INPUT_SIV => {
                    let register = declared_register(tokens, &insn)?;
                    max_input = Some(max_input.unwrap_or(0).max(register));
                    if insn.opcode == OPCODE_DCL_INPUT_SGV
                        && tokens[insn.end - 1] == SB_NAME_INSTANCE_ID
                    {
                        return Err(DxbcError::InstanceIdAlreadyDeclared);
                    }
                }
                OPCODE_DCL_OUTPUT | OPCODE_DCL_OUTPUT_SGV | OPCODE_DCL_OUTPUT_SIV => {
                    let register = declared_register(tokens, &insn)?;
                    max_output = Some(max_output.unwrap_or(0).max(register));
                }
                OPCODE_DCL_TEMPS => {
                    temps = *tokens
                        .get(insn.start + 1)
                        .ok_or(DxbcError::UnexpectedEndOfTokens)?;
                    dcl_temps_start = Some(insn.start);
                }
                OPCODE_DCL_CONSTANT_BUFFER
                    if declared_register(tokens, &insn)? == STEREO_CB_REGISTER =>
                {
                    return Err(DxbcError::Cb13AlreadyDeclared);
                }
                _ => {}
            }
        } else {
            if inject_before.is_none() {
                inject_before = Some(insn.start);
            }
            for operand in insn.operands() {
                if is_per_eye_operand(&operand?.kind) {
                    per_eye_found = true;
                }
            }
        }
    }

    if !per_eye_found {
        return Err(DxbcError::NoPerEyeReferences);
    }
    Ok(RewritePlan {
        input_register: max_input.map_or(0, |r| r + 1),
        output_register: max_output.map_or(0, |r| r + 1),
        temp_register: temps,
        dcl_temps_start,
        inject_before: inject_before.unwrap_or(tokens.len()),
    })
}

/// Rebuilds the shader chunk: injects the new declarations and the prologue at the plan's boundary,
/// bumps `dcl_temps`, and remaps every per-eye `cb0` operand to `cb13[rBase.x + k]`. Returns the
/// chunk body bytes with the length dword updated.
fn rewrite_shader(stream: &TokenStream, plan: &RewritePlan) -> Result<Vec<u8>, DxbcError> {
    let tokens = stream.tokens();
    let mut out: Vec<u32> = Vec::with_capacity(tokens.len() + 64);
    out.extend_from_slice(&tokens[..2]);

    let mut injected = false;
    for insn in stream.instructions() {
        let insn = insn?;
        if insn.start == plan.inject_before {
            emit_injection(&mut out, plan);
            injected = true;
        }
        if Some(insn.start) == plan.dcl_temps_start {
            out.push(tokens[insn.start]);
            out.push(plan.temp_register + 1);
            continue;
        }
        if insn.is_declaration() {
            out.extend_from_slice(&tokens[insn.start..insn.end]);
            continue;
        }
        rewrite_instruction(tokens, &insn, plan.temp_register, &mut out)?;
    }
    if !injected {
        emit_injection(&mut out, plan);
    }

    let length = out.len() as u32;
    out[1] = length;
    Ok(out.iter().flat_map(|t| t.to_le_bytes()).collect())
}

/// Emits the new declarations and the eye prologue. Ordering note: the viewport `mov` reads the eye
/// *before* the `imul` overwrites the temp with the eye's `cb13` row base, so one temp suffices.
fn emit_injection(out: &mut Vec<u32>, plan: &RewritePlan) {
    let n = plan.input_register;
    let m = plan.output_register;
    let r = plan.temp_register;
    // dcl_constantbuffer CB13[10], dynamicIndexed.
    out.extend_from_slice(&[
        (4 << 24) | CB_ACCESS_DYNAMIC_INDEXED | OPCODE_DCL_CONSTANT_BUFFER,
        OPERAND_CB_2D_IMM,
        STEREO_CB_REGISTER,
        STEREO_CB_ROWS,
    ]);
    // dcl_input_sgv vN.x, instance_id.
    out.extend_from_slice(&[
        (4 << 24) | OPCODE_DCL_INPUT_SGV,
        OPERAND_INPUT_MASK_X,
        n,
        SB_NAME_INSTANCE_ID,
    ]);
    // dcl_output_siv oM.x, viewport_array_index.
    out.extend_from_slice(&[
        (4 << 24) | OPCODE_DCL_OUTPUT_SIV,
        OPERAND_OUTPUT_MASK_X,
        m,
        SB_NAME_VIEWPORT_ARRAY_INDEX,
    ]);
    if plan.dcl_temps_start.is_none() {
        out.extend_from_slice(&[(2 << 24) | OPCODE_DCL_TEMPS, r + 1]);
    }
    // and rBase.x, vN.x, l(1) -- the eye index.
    out.extend_from_slice(&[
        (7 << 24) | OPCODE_AND,
        OPERAND_TEMP_MASK_X,
        r,
        OPERAND_INPUT_SELECT_X,
        n,
        OPERAND_IMM32_SCALAR,
        1,
    ]);
    // mov oM.x, rBase.x -- route to the eye's viewport.
    out.extend_from_slice(&[
        (5 << 24) | OPCODE_MOV,
        OPERAND_OUTPUT_MASK_X,
        m,
        OPERAND_TEMP_SELECT_X,
        r,
    ]);
    // imul null, rBase.x, rBase.x, l(5) -- the eye's cb13 row base.
    out.extend_from_slice(&[
        (8 << 24) | OPCODE_IMUL,
        OPERAND_NULL,
        OPERAND_TEMP_MASK_X,
        r,
        OPERAND_TEMP_SELECT_X,
        r,
        OPERAND_IMM32_SCALAR,
        5,
    ]);
}

/// Re-serializes one instruction, remapping any per-eye `cb0[row]` operand to `cb13[rBase.x + k]`:
/// the element index becomes immediate-plus-relative with a two-token `rBase.x` operand appended,
/// so the operand grows and the instruction's length field is recomputed.
fn rewrite_instruction(
    tokens: &[u32],
    insn: &Instruction<'_>,
    temp_register: u32,
    out: &mut Vec<u32>,
) -> Result<(), DxbcError> {
    let opcode_at = out.len();
    out.extend_from_slice(&tokens[insn.start..insn.operands_start()]);

    let mut pos = insn.operands_start();
    while pos < insn.end {
        let (operand, next) = parse_operand(tokens, pos)?;
        if is_per_eye_operand(&operand.kind) {
            let row = match operand.kind {
                OperandKind::ConstantBuffer { element, .. } => element,
                _ => unreachable!("is_per_eye_operand only matches immediate cb0 refs"),
            };
            let tok = tokens[pos];
            // Both index dimensions must be plain 32-bit immediates: the register index and the
            // element index each occupy one token, at fixed offsets past any extended operand.
            if (tok >> 22) & 0x7 != 0 || (tok >> 25) & 0x7 != 0 {
                return Err(DxbcError::UnsupportedOperandEncoding);
            }
            let extended = ((tok >> 31) & 1) as usize;
            out.push(tok | (IDX_IMM32_PLUS_RELATIVE << 25));
            out.extend_from_slice(&tokens[pos + 1..pos + 1 + extended]);
            out.push(STEREO_CB_REGISTER);
            out.push(if row == 4 { 4 } else { row - 29 });
            out.push(OPERAND_TEMP_SELECT_X);
            out.push(temp_register);
        } else {
            out.extend_from_slice(&tokens[pos..next]);
        }
        pos = next;
    }

    let new_len = (out.len() - opcode_at) as u32;
    if new_len > 0x7F {
        return Err(DxbcError::InstructionTooLong);
    }
    out[opcode_at] = (out[opcode_at] & !(0x7F << 24)) | (new_len << 24);
    Ok(())
}

/// Whether an operand is a per-eye `cb0` row reference (an immediate index into one of
/// [`PER_EYE_CB0_ROWS`]).
fn is_per_eye_operand(kind: &OperandKind) -> bool {
    matches!(
        kind,
        OperandKind::ConstantBuffer {
            register: 0,
            element,
        } if PER_EYE_CB0_ROWS.contains(element)
    )
}

/// Reads the register index of a declaration's operand (`dcl_input v2`, `dcl_constantbuffer
/// cb0[38]`, ...): the first index token after the operand token and any extended operand token.
fn declared_register(tokens: &[u32], insn: &Instruction<'_>) -> Result<u32, DxbcError> {
    let at = insn.start + 1;
    let tok = *tokens.get(at).ok_or(DxbcError::UnexpectedEndOfTokens)?;
    let extended = ((tok >> 31) & 1) as usize;
    tokens
        .get(at + 1 + extended)
        .copied()
        .ok_or(DxbcError::UnexpectedEndOfTokens)
}

/// One `ISGN`/`OSGN` element to append.
struct SignatureElement<'a> {
    name: &'a str,
    semantic_index: u32,
    system_value: u32,
    component_type: u32,
    register: u32,
    mask: u8,
    /// The read mask for inputs, or the never-written mask for outputs.
    rw_mask: u8,
}

/// The fixed size of an `ISGN`/`OSGN` element record.
const SIGNATURE_ELEMENT_LEN: usize = 24;

/// Appends an element to an `ISGN`/`OSGN` chunk body: the element table grows by one record (which
/// shifts every name offset), the new semantic name lands at the end of the string blob, and the
/// chunk is re-padded to a dword boundary with `0xAB` as fxc does.
fn append_signature_element(
    body: &[u8],
    element: &SignatureElement<'_>,
) -> Result<Vec<u8>, DxbcError> {
    if body.len() < 8 {
        return Err(DxbcError::MalformedSignature);
    }
    let count = u32::from_le_bytes(body[0..4].try_into().expect("4 bytes")) as usize;
    let table_offset = u32::from_le_bytes(body[4..8].try_into().expect("4 bytes")) as usize;
    let strings_start = table_offset
        .checked_add(count * SIGNATURE_ELEMENT_LEN)
        .ok_or(DxbcError::MalformedSignature)?;
    if table_offset != 8 || strings_start > body.len() {
        return Err(DxbcError::MalformedSignature);
    }
    // Strip fxc's 0xAB tail padding so the new name appends directly after the last string; 0xAB
    // never occurs inside the ASCII semantic names.
    let strings = {
        let mut s = &body[strings_start..];
        while let [rest @ .., 0xAB] = s {
            s = rest;
        }
        s
    };

    let mut out = Vec::with_capacity(body.len() + SIGNATURE_ELEMENT_LEN + element.name.len() + 4);
    out.extend_from_slice(&(count as u32 + 1).to_le_bytes());
    out.extend_from_slice(&(table_offset as u32).to_le_bytes());
    for i in 0..count {
        let record = &body[table_offset + i * SIGNATURE_ELEMENT_LEN..][..SIGNATURE_ELEMENT_LEN];
        let name_offset = u32::from_le_bytes(record[0..4].try_into().expect("4 bytes"));
        out.extend_from_slice(&(name_offset + SIGNATURE_ELEMENT_LEN as u32).to_le_bytes());
        out.extend_from_slice(&record[4..]);
    }
    let new_name_offset = table_offset + (count + 1) * SIGNATURE_ELEMENT_LEN + strings.len();
    out.extend_from_slice(&(new_name_offset as u32).to_le_bytes());
    out.extend_from_slice(&element.semantic_index.to_le_bytes());
    out.extend_from_slice(&element.system_value.to_le_bytes());
    out.extend_from_slice(&element.component_type.to_le_bytes());
    out.extend_from_slice(&element.register.to_le_bytes());
    out.push(element.mask);
    out.push(element.rw_mask);
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(strings);
    out.extend_from_slice(element.name.as_bytes());
    out.push(0);
    while out.len() % 4 != 0 {
        out.push(0xAB);
    }
    Ok(out)
}

/// An `SFI0` body with the viewport-from-any-stage feature bit ORed in (created from scratch when
/// the container had no `SFI0`).
fn with_viewport_feature_bit(existing: &[u8]) -> Vec<u8> {
    let mut body = existing.to_vec();
    if body.len() < 8 {
        body.resize(8, 0);
    }
    let flags = u64::from_le_bytes(body[0..8].try_into().expect("8 bytes"));
    body[0..8].copy_from_slice(&(flags | SFI0_VIEWPORT_FROM_ANY_STAGE).to_le_bytes());
    body
}

/// Assembles a DXBC container from chunks in order: header (magic, zeroed digest for
/// [`refresh_checksum`] to fill, version 1, total size, chunk count), the offset table, then the
/// chunks.
fn build_container(chunks: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let table_len = 0x20 + chunks.len() * 4;
    let total: usize = table_len + chunks.iter().map(|(_, body)| 8 + body.len()).sum::<usize>();

    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"DXBC");
    out.extend_from_slice(&[0u8; 16]);
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(chunks.len() as u32).to_le_bytes());
    let mut offset = table_len;
    for (_, body) in chunks {
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += 8 + body.len();
    }
    for (tag, body) in chunks {
        out.extend_from_slice(tag);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
    }
    out
}
