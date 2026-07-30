//! The `M_eye` reprojection vertex transform for single-pass stereo.
//!
//! Targets the baked-WVP model families -- NPCs, props, and buildings -- whose vertex shaders write
//! `SV_Position` from a scene view-projection without exposing per-eye `cb0` rows to remap. Instead
//! of remapping operands, it saves the shader's own center-clip position and post-multiplies it by a
//! per-eye matrix `M_eye = VP_eye · VP_center⁻¹` before each `ret`, so each eye lands its own clip
//! position. The eye index, viewport routing, and `cb13` layout are shared with the terrain domain
//! reprojection.

use crate::{
    container::{Dxbc, DxbcError},
    rewrite::common::{
        CB_ACCESS_DYNAMIC_INDEXED, OPCODE_AND, OPCODE_DCL_CONSTANT_BUFFER, OPCODE_DCL_INPUT_SGV,
        OPCODE_DCL_OUTPUT_SIV, OPCODE_DCL_TEMPS, OPCODE_IMUL, OPCODE_MOV, OPCODE_RET, OPCODE_RETC,
        OPERAND_CB_2D_IMM, OPERAND_IMM32_SCALAR, OPERAND_INPUT_MASK_X, OPERAND_INPUT_SELECT_X,
        OPERAND_NULL, OPERAND_OUTPUT_MASK_X, OPERAND_TEMP_MASK_X, OPERAND_TEMP_SELECT_X,
        SB_NAME_INSTANCE_ID, SB_NAME_POSITION, SB_NAME_VIEWPORT_ARRAY_INDEX,
        SIGNATURE_COMPONENT_UINT32, SIGNATURE_SYSVALUE_INSTANCE_ID,
        SIGNATURE_SYSVALUE_VIEWPORT_ARRAY_INDEX, STEREO_CB_REGISTER, STEREO_REPROJ_CB_ROWS,
        SignatureElement, append_signature_element, declared_register, emit_meye_epilogue,
        max_signature_register, reassemble, rewrite_reproject_instruction,
    },
    tokens::{ShaderStage, TokenStream},
};

/// Rewrites a vertex shader for single-pass stereo by **reprojection**: instead of remapping `cb0`
/// per-eye operands (which the baked-WVP / terrain / GPU-indirect families don't have), it renames
/// the shader's own `SV_Position` writes to a temp `rClip` and post-multiplies by a per-eye matrix
/// `M_eye` before each `ret`, so `o0 = M_eye · clip_center` lands the shader's own center-clip
/// position in each eye. `M_eye = VP_eye · VP_center⁻¹` is uploaded to `cb13`'s four-rows-per-eye
/// block at [`MEYE_ROW_BASE`]; the eye index, viewport routing, `SV_InstanceID`/`SV_ViewportArrayIndex`
/// interface, `SFI0` bit, and checksum are the same as the remap. Shader-agnostic: it works on any
/// vertex shader that writes `SV_Position` from a scene view-projection, whatever buffer that came
/// from. The runtime gate (scene pass range ∩ `DrawIndexed`) is what excludes NDC-direct writers
/// (sky, UI), which the bytecode can't distinguish.
pub fn reproject_vertex_shader(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
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
    let plan = plan_reproject(&stream, input_register, output_register)?;
    let new_shex = rewrite_reproject(&stream, &plan)?;

    let new_isgn = append_signature_element(
        isgn.body(blob),
        &SignatureElement {
            name: "SV_InstanceID",
            semantic_index: 0,
            system_value: SIGNATURE_SYSVALUE_INSTANCE_ID,
            component_type: SIGNATURE_COMPONENT_UINT32,
            register: plan.input_register,
            mask: 0x01,
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
            rw_mask: 0x0E,
        },
    )?;

    reassemble(&dxbc, blob, new_isgn, new_osgn, new_shex, true)
}

/// What the reprojection declaration scan learned: the `SV_Position` output to reproject, the free
/// registers, the temp base (`rBase` = base, `rClip` = base + 1), and the injection point.
struct ReprojectPlan {
    /// The output register `SV_Position` is declared at (`dcl_output_siv oN, position`).
    pos_register: u32,
    /// The `v` register for the new `SV_InstanceID` input.
    input_register: u32,
    /// The `o` register for the new `SV_ViewportArrayIndex` output.
    output_register: u32,
    /// The first of the two temps this rewrite claims: `rBase` (eye row base) at `temp_base`, `rClip`
    /// (saved clip position) at `temp_base + 1`.
    temp_base: u32,
    /// The token index of the existing `dcl_temps`, if any (its count is bumped by two).
    dcl_temps_start: Option<usize>,
    /// The token index of the first non-declaration instruction, where declarations and the prologue
    /// are injected.
    inject_before: usize,
}

/// Scans the declarations for the reprojection rewrite: the `SV_Position` output register, the temp
/// count and injection point, and the preconditions (`cb13` and `SV_InstanceID` unclaimed, an
/// `SV_Position` output present, no `retc`). The free input/output registers are supplied by the
/// caller from the signatures, which are authoritative over the declarations.
fn plan_reproject(
    stream: &TokenStream,
    input_register: u32,
    output_register: u32,
) -> Result<ReprojectPlan, DxbcError> {
    let tokens = stream.tokens();
    let mut temps: u32 = 0;
    let mut dcl_temps_start = None;
    let mut inject_before = None;
    let mut pos_register = None;

    for insn in stream.instructions() {
        let insn = insn?;
        if insn.is_declaration() {
            match insn.opcode {
                OPCODE_DCL_INPUT_SGV if tokens[insn.end - 1] == SB_NAME_INSTANCE_ID => {
                    return Err(DxbcError::InstanceIdAlreadyDeclared);
                }
                OPCODE_DCL_OUTPUT_SIV if tokens[insn.end - 1] == SB_NAME_POSITION => {
                    pos_register = Some(declared_register(tokens, &insn)?);
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
            // A conditional early return would need the per-eye epilogue on that path too.
            if insn.opcode == OPCODE_RETC {
                return Err(DxbcError::UnsupportedControlFlow);
            }
        }
    }

    let pos_register = pos_register.ok_or(DxbcError::NoPositionOutput)?;
    Ok(ReprojectPlan {
        pos_register,
        input_register,
        output_register,
        temp_base: temps,
        dcl_temps_start,
        inject_before: inject_before.unwrap_or(tokens.len()),
    })
}

/// Rebuilds the shader chunk for reprojection: injects the declarations and eye prologue, renames
/// every `SV_Position` write to the `rClip` temp, and emits the `M_eye · rClip` `dp4` chain before
/// each `ret`.
fn rewrite_reproject(stream: &TokenStream, plan: &ReprojectPlan) -> Result<Vec<u8>, DxbcError> {
    let tokens = stream.tokens();
    let mut out: Vec<u32> = Vec::with_capacity(tokens.len() + 128);
    out.extend_from_slice(&tokens[..2]);

    let mut injected = false;
    for insn in stream.instructions() {
        let insn = insn?;
        if insn.start == plan.inject_before {
            emit_reproject_injection(&mut out, plan);
            injected = true;
        }
        if Some(insn.start) == plan.dcl_temps_start {
            out.push(tokens[insn.start]);
            out.push(plan.temp_base + 2);
            continue;
        }
        if insn.is_declaration() {
            out.extend_from_slice(&tokens[insn.start..insn.end]);
            continue;
        }
        if insn.opcode == OPCODE_RET {
            emit_meye_epilogue(&mut out, plan.pos_register, plan.temp_base);
            out.extend_from_slice(&tokens[insn.start..insn.end]);
            continue;
        }
        rewrite_reproject_instruction(tokens, &insn, plan.pos_register, plan.temp_base, &mut out)?;
    }
    if !injected {
        emit_reproject_injection(&mut out, plan);
    }

    let length = out.len() as u32;
    out[1] = length;
    Ok(out.iter().flat_map(|t| t.to_le_bytes()).collect())
}

/// Emits the reprojection declarations and eye prologue: `cb13[18]`, the `SV_InstanceID` input, the
/// `SV_ViewportArrayIndex` output, then `and rBase.x, vN.x, l(1)` (eye), `mov oM.x, rBase.x`
/// (viewport), `imul null, rBase.x, rBase.x, l(4)` (the eye's `M_eye` row base, four rows per eye).
fn emit_reproject_injection(out: &mut Vec<u32>, plan: &ReprojectPlan) {
    let n = plan.input_register;
    let m = plan.output_register;
    let r = plan.temp_base;
    out.extend_from_slice(&[
        (4 << 24) | CB_ACCESS_DYNAMIC_INDEXED | OPCODE_DCL_CONSTANT_BUFFER,
        OPERAND_CB_2D_IMM,
        STEREO_CB_REGISTER,
        STEREO_REPROJ_CB_ROWS,
    ]);
    out.extend_from_slice(&[
        (4 << 24) | OPCODE_DCL_INPUT_SGV,
        OPERAND_INPUT_MASK_X,
        n,
        SB_NAME_INSTANCE_ID,
    ]);
    out.extend_from_slice(&[
        (4 << 24) | OPCODE_DCL_OUTPUT_SIV,
        OPERAND_OUTPUT_MASK_X,
        m,
        SB_NAME_VIEWPORT_ARRAY_INDEX,
    ]);
    if plan.dcl_temps_start.is_none() {
        out.extend_from_slice(&[(2 << 24) | OPCODE_DCL_TEMPS, r + 2]);
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
    // imul null, rBase.x, rBase.x, l(4) -- the eye's M_eye row base (four rows per eye).
    out.extend_from_slice(&[
        (8 << 24) | OPCODE_IMUL,
        OPERAND_NULL,
        OPERAND_TEMP_MASK_X,
        r,
        OPERAND_TEMP_SELECT_X,
        r,
        OPERAND_IMM32_SCALAR,
        4,
    ]);
}
