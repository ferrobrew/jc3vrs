//! The `cb0`-remap vertex transform for single-pass stereo.
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
    container::{Dxbc, DxbcError},
    tokens::{
        IDX_IMM32_PLUS_RELATIVE, Instruction, OperandKind, ShaderStage, TokenStream, parse_operand,
    },
};

use super::common::{
    CB_ACCESS_DYNAMIC_INDEXED, OPCODE_AND, OPCODE_DCL_CONSTANT_BUFFER, OPCODE_DCL_INPUT_SGV,
    OPCODE_DCL_OUTPUT_SIV, OPCODE_DCL_TEMPS, OPCODE_IMUL, OPCODE_MOV, OPERAND_CB_2D_IMM,
    OPERAND_IMM32_SCALAR, OPERAND_INPUT_MASK_X, OPERAND_INPUT_SELECT_X, OPERAND_NULL,
    OPERAND_OUTPUT_MASK_X, OPERAND_TEMP_MASK_X, OPERAND_TEMP_SELECT_X, SB_NAME_INSTANCE_ID,
    SB_NAME_VIEWPORT_ARRAY_INDEX, SIGNATURE_COMPONENT_UINT32, SIGNATURE_SYSVALUE_INSTANCE_ID,
    SIGNATURE_SYSVALUE_VIEWPORT_ARRAY_INDEX, STEREO_CB_REGISTER, STEREO_CB_ROWS, SignatureElement,
    append_signature_element, declared_register, max_signature_register, reassemble,
};

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

    // The free input/output registers must come from the signatures, not the body's `dcl_input`/
    // `dcl_output` opcodes: fxc keeps an `ISGN`/`OSGN` element for an input the shader never reads
    // (`ReadWriteMask = 0`) and emits no declaration for it, so a declaration scan alone under-counts
    // and the appended element collides with that dead slot.
    let input_register = max_signature_register(isgn.body(blob)).map_or(0, |r| r + 1);
    let output_register = max_signature_register(osgn.body(blob)).map_or(0, |r| r + 1);
    let plan = plan_rewrite(&stream, input_register, output_register)?;
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

    reassemble(&dxbc, blob, new_isgn, new_osgn, new_shex, true)
}

/// What the declaration scan learned: where to inject, and which registers are free.
struct RewritePlan {
    /// The `v` register for the new `SV_InstanceID` input (max `ISGN` register + 1).
    input_register: u32,
    /// The `o` register for the new `SV_ViewportArrayIndex` output (max `OSGN` register + 1).
    output_register: u32,
    /// The temp holding the eye and then the eye's `cb13` row base (the old `dcl_temps` count).
    temp_register: u32,
    /// The token index of the existing `dcl_temps` instruction, if any (its count is bumped).
    dcl_temps_start: Option<usize>,
    /// The token index of the first non-declaration instruction -- the injection point for the new
    /// declarations and the prologue.
    inject_before: usize,
}

/// Scans the declarations: the temp count, the injection point, and the preconditions (`cb13` and
/// `SV_InstanceID` unclaimed, per-eye rows present). The free input/output registers are supplied by
/// the caller from the signatures, which are authoritative over the declarations.
fn plan_rewrite(
    stream: &TokenStream,
    input_register: u32,
    output_register: u32,
) -> Result<RewritePlan, DxbcError> {
    let tokens = stream.tokens();
    let mut temps: u32 = 0;
    let mut dcl_temps_start = None;
    let mut inject_before = None;
    let mut per_eye_found = false;

    for insn in stream.instructions() {
        let insn = insn?;
        if insn.is_declaration() {
            match insn.opcode {
                OPCODE_DCL_INPUT_SGV if tokens[insn.end - 1] == SB_NAME_INSTANCE_ID => {
                    return Err(DxbcError::InstanceIdAlreadyDeclared);
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
        input_register,
        output_register,
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
        let row = match &operand.kind {
            OperandKind::ConstantBuffer {
                register: 0,
                element,
            } if PER_EYE_CB0_ROWS.contains(element) => *element,
            _ => {
                out.extend_from_slice(&tokens[pos..next]);
                pos = next;
                continue;
            }
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
