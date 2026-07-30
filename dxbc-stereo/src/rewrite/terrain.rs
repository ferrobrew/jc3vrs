//! The terrain tessellation pipeline transforms for single-pass stereo.
//!
//! Handles the two ends of the terrain tessellation path: the vertex shader originates the eye index
//! and rides it through the free `.z` of its `TEXCOORD3` output lane (VS → HS → DS), and the domain
//! shader reads that lane, reprojects its own `SV_Position` via the per-eye `M_eye`, and routes to
//! the eye's viewport. Both share the `M_eye` epilogue and `cb13` layout with the vertex
//! reprojection.

use crate::{
    container::{Dxbc, DxbcError},
    rewrite::common::{
        CB_ACCESS_DYNAMIC_INDEXED, OPCODE_AND, OPCODE_DCL_CONSTANT_BUFFER, OPCODE_DCL_INPUT,
        OPCODE_DCL_INPUT_SGV, OPCODE_DCL_OUTPUT, OPCODE_DCL_OUTPUT_SGV, OPCODE_DCL_OUTPUT_SIV,
        OPCODE_DCL_TEMPS, OPCODE_IMUL, OPCODE_MOV, OPCODE_RET, OPCODE_RETC, OPERAND_CB_2D_IMM,
        OPERAND_IMM32_SCALAR, OPERAND_INPUT_MASK_X, OPERAND_INPUT_SELECT_X, OPERAND_NULL,
        OPERAND_OUTPUT_MASK_X, OPERAND_OUTPUT_MASK_Z, OPERAND_TEMP_MASK_X, OPERAND_TEMP_SELECT_X,
        OPERAND_TYPE_OUTPUT, SB_NAME_INSTANCE_ID, SB_NAME_POSITION, SB_NAME_VIEWPORT_ARRAY_INDEX,
        SIGNATURE_COMPONENT_UINT32, SIGNATURE_SYSVALUE_INSTANCE_ID,
        SIGNATURE_SYSVALUE_VIEWPORT_ARRAY_INDEX, STEREO_CB_REGISTER, STEREO_REPROJ_CB_ROWS,
        SignatureElement, append_signature_element, declared_register, emit_meye_epilogue,
        max_signature_register, reassemble, rewrite_reproject_instruction, signature_register,
        widen_signature_mask,
    },
    tokens::{Instruction, ShaderStage, TokenStream},
};

/// The output semantic that carries the single-pass eye index through the terrain tessellation
/// pipeline: `TEXCOORD3`'s free `.z` component (the VS uses only `.xy`). The VS writes the eye there,
/// the hull shader forwards it, and the domain shader reads it to reproject and route.
const EYE_LANE_SEMANTIC: &str = "TEXCOORD";
const EYE_LANE_SEMANTIC_INDEX: u32 = 3;
/// The `.z` write-mask bit in an operand token's mask nibble (bit 6 of the low byte). ORing it into a
/// `dcl_output`/`mov` operand widens `.xy` to `.xyz`.
const MASK_BIT_Z: u32 = 0x40;

/// SM5 operand type: domain-shader input control point (`vicp[cp][reg]`). Identifies the lane
/// declaration to widen in the terrain domain reprojection.
const OPERAND_TYPE_INPUT_CONTROL_POINT: u32 = 25;
/// SM4/5 operand type: input register (`vN`, or `v[cp][reg]` in the hull control-point phase).
/// Identifies the lane declaration to widen in the hull shader.
const OPERAND_TYPE_INPUT: u32 = 1;

/// SM5 hull-shader phase markers. The control-point phase forwards each output control point from the
/// matching input control point (where the eye lane is widened); the fork and join phases compute
/// tessellation factors and must be left untouched (they reuse the same `o` registers for unrelated
/// system values).
const OPCODE_HS_CONTROL_POINT_PHASE: u32 = 0x72;
const OPCODE_HS_FORK_PHASE: u32 = 0x73;
const OPCODE_HS_JOIN_PHASE: u32 = 0x74;
/// The two-bit component selector for `.z` in an operand's swizzle field (component 2, at bits `[9:8]`
/// of the operand token). The hull passthrough copies the lane with a swizzle; widening it to carry
/// the eye means forcing its third swizzle component to `.z`.
const SWIZZLE_Z_COMPONENT_2: u32 = 0x2 << 8;
const SWIZZLE_COMPONENT_2_MASK: u32 = 0x3 << 8;
/// `vicp[0][reg].z` as a source: input-control-point register, 2D immediate index, select-1 component
/// `.z`. The two index tokens (control point `0`, register `reg`) follow. The domain reprojection reads
/// the eye from the free `.z` of the `TEXCOORD3` control-point lane.
// Operand token layout: bits 12..20 = 25 (operand type: input-control-point), bits 20..22 = 2 (2D
// index dimension), bits 4..6 = 1 (select-1 component mode), bits 0..2 = 2 (the selected component: .z).
const OPERAND_VICP_SELECT_Z: u32 = 0x0021_902A;

/// Rewrites the terrain tessellation **vertex** shader to originate the single-pass eye index: adds an
/// `SV_InstanceID` input and writes `eye = id & 1` into the free `.z` of its `TEXCOORD3` output lane,
/// widening that output to `.xyz`. No reprojection and no viewport output -- a tessellation VS has no
/// `SV_Position` (the domain shader builds clip), and only the last pre-rasterization stage may write
/// `SV_ViewportArrayIndex`. The eye rides `TEXCOORD3.z` through the hull shader
/// ([`forward_eye_hull_shader`]) to the domain shader ([`reproject_domain_shader`]).
pub fn inject_eye_forward_vertex_shader(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
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

    // The eye rides TEXCOORD3.z: confirm the output exists and its .z is free (mask is exactly .xy).
    let (lane_register, lane_mask) =
        signature_register(osgn.body(blob), EYE_LANE_SEMANTIC, EYE_LANE_SEMANTIC_INDEX)
            .ok_or(DxbcError::NoEyeLane)?;
    if lane_mask & 0x4 != 0 {
        return Err(DxbcError::NoEyeLane);
    }

    // The free input register must come from the input signature, not the body's `dcl_input` opcodes:
    // fxc keeps an `ISGN` element for an input the shader never reads (`ReadWriteMask = 0`) and emits
    // no declaration for it, so a declaration scan alone under-counts and `SV_InstanceID` would land
    // on that dead slot.
    let input_register = max_signature_register(isgn.body(blob)).map_or(0, |r| r + 1);
    let plan = plan_eye_inject(&stream, lane_register, input_register)?;
    let new_shex = rewrite_eye_inject(&stream, &plan)?;

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
    // Widen the lane output to .xyz (the .z now carries the eye). The rw-mask is the never-written
    // mask: .xyz are written, so only .w is never written.
    let new_osgn = widen_signature_mask(
        osgn.body(blob),
        EYE_LANE_SEMANTIC,
        EYE_LANE_SEMANTIC_INDEX,
        lane_mask | 0x4,
        0x8,
    )?;

    reassemble(&dxbc, blob, new_isgn, new_osgn, new_shex, false)
}

/// Rewrites the terrain tessellation **hull** shader to forward the single-pass eye index the vertex
/// shader originated on `TEXCOORD3.z`. The hull control-point phase copies each output control point
/// from its input; this widens the `TEXCOORD3` lane it forwards -- the input `v[..][2]` declaration,
/// the output `o<lane>` declaration, and the passthrough `mov o<lane>, v[..][lane]` -- from `.xy` to
/// `.xyz`, so the eye survives VS -> HS -> DS. The fork and join phases (tessellation factors) are left
/// untouched, since they reuse the same `o` registers for unrelated system values. No reprojection and
/// no viewport: the hull shader neither builds clip nor feeds the rasterizer.
pub fn forward_eye_hull_shader(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
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
    if stream.stage() != ShaderStage::Hull {
        return Err(DxbcError::NotHullShader);
    }

    // The eye rides TEXCOORD3.z: confirm the input lane exists and its .z is free (mask is .xy).
    let (lane_register, lane_mask) =
        signature_register(isgn.body(blob), EYE_LANE_SEMANTIC, EYE_LANE_SEMANTIC_INDEX)
            .ok_or(DxbcError::NoEyeLane)?;
    if lane_mask & 0x4 != 0 {
        return Err(DxbcError::NoEyeLane);
    }
    let (_, osgn_lane_mask) =
        signature_register(osgn.body(blob), EYE_LANE_SEMANTIC, EYE_LANE_SEMANTIC_INDEX)
            .ok_or(DxbcError::NoEyeLane)?;

    let plan = plan_eye_forward(&stream, lane_register)?;
    let new_shex = rewrite_eye_forward(&stream, &plan)?;

    // Widen the forwarded lane on both interfaces: the input (from the VS) and the output (to the DS),
    // both to `.xyz`. For the input, the mask and used-mask both gain `.z`; for the output, the mask
    // gains `.z` and the never-written mask drops it (only `.w` stays unwritten).
    let new_isgn = widen_signature_mask(
        isgn.body(blob),
        EYE_LANE_SEMANTIC,
        EYE_LANE_SEMANTIC_INDEX,
        lane_mask | 0x4,
        lane_mask | 0x4,
    )?;
    let new_osgn = widen_signature_mask(
        osgn.body(blob),
        EYE_LANE_SEMANTIC,
        EYE_LANE_SEMANTIC_INDEX,
        osgn_lane_mask | 0x4,
        0x8,
    )?;

    reassemble(&dxbc, blob, new_isgn, new_osgn, new_shex, false)
}

/// What the hull-shader eye-forward scan learned: the token indices of the three control-point-phase
/// items whose `TEXCOORD3` lane is widened to `.xyz`.
struct EyeForwardPlan {
    /// The token index of the control-point-phase `dcl_input v[..][lane]`, whose mask gains `.z`.
    lane_input_dcl: usize,
    /// The token index of the control-point-phase `dcl_output o<lane>`, whose mask gains `.z`.
    lane_output_dcl: usize,
    /// The token index of the passthrough `mov o<lane>, v[..][lane]`, whose destination mask gains `.z`
    /// and whose source swizzle's third component is forced to `.z`.
    lane_mov: usize,
}

/// Scans the hull control-point phase for the three lane items to widen. Ignores the fork and join
/// phases entirely -- they reuse the same `o` registers for tessellation factors.
fn plan_eye_forward(stream: &TokenStream, lane_register: u32) -> Result<EyeForwardPlan, DxbcError> {
    let tokens = stream.tokens();
    let mut in_control_point_phase = false;
    let mut lane_input_dcl = None;
    let mut lane_output_dcl = None;
    let mut lane_mov = None;

    for insn in stream.instructions() {
        let insn = insn?;
        match insn.opcode {
            OPCODE_HS_CONTROL_POINT_PHASE => in_control_point_phase = true,
            OPCODE_HS_FORK_PHASE | OPCODE_HS_JOIN_PHASE => in_control_point_phase = false,
            _ => {}
        }
        if !in_control_point_phase {
            continue;
        }
        match insn.opcode {
            OPCODE_DCL_INPUT
                if two_d_input_register(tokens, &insn, OPERAND_TYPE_INPUT)
                    == Some(lane_register) =>
            {
                lane_input_dcl = Some(insn.start);
            }
            OPCODE_DCL_OUTPUT if declared_register(tokens, &insn)? == lane_register => {
                lane_output_dcl = Some(insn.start);
            }
            OPCODE_MOV if mov_output_destination(tokens, &insn) == Some(lane_register) => {
                lane_mov = Some(insn.start);
            }
            _ => {}
        }
    }

    Ok(EyeForwardPlan {
        lane_input_dcl: lane_input_dcl.ok_or(DxbcError::NoEyeLane)?,
        lane_output_dcl: lane_output_dcl.ok_or(DxbcError::NoEyeLane)?,
        lane_mov: lane_mov.ok_or(DxbcError::NoEyeLane)?,
    })
}

/// The output register a `mov` writes to, or `None` if its destination is not a one-dimensionally
/// indexed output register. Used to find the hull passthrough `mov` for the lane (the destination is
/// the first operand, at the instruction's operand start).
fn mov_output_destination(tokens: &[u32], insn: &Instruction<'_>) -> Option<u32> {
    let at = insn.operands_start();
    let tok = *tokens.get(at)?;
    if (tok >> 12) & 0xFF != OPERAND_TYPE_OUTPUT || (tok >> 20) & 0x3 != 1 {
        return None;
    }
    let extended = ((tok >> 31) & 1) as usize;
    tokens.get(at + 1 + extended).copied()
}

/// Rebuilds the hull chunk, widening the three control-point-phase lane items to `.xyz`: the input and
/// output lane declarations (mask bit), and the passthrough `mov` (destination mask bit, plus its
/// source swizzle's third component forced to `.z` so the copied `.z` is the eye, not a repeat of `.x`).
fn rewrite_eye_forward(stream: &TokenStream, plan: &EyeForwardPlan) -> Result<Vec<u8>, DxbcError> {
    let tokens = stream.tokens();
    let mut out: Vec<u32> = Vec::with_capacity(tokens.len());
    out.extend_from_slice(&tokens[..2]);

    for insn in stream.instructions() {
        let insn = insn?;
        if insn.start == plan.lane_input_dcl || insn.start == plan.lane_output_dcl {
            // Widen the lane declaration `.xy` -> `.xyz` (mask bit on the operand token).
            out.push(tokens[insn.start]);
            out.push(tokens[insn.start + 1] | MASK_BIT_Z);
            out.extend_from_slice(&tokens[insn.start + 2..insn.end]);
            continue;
        }
        if insn.start == plan.lane_mov {
            let base = out.len();
            out.extend_from_slice(&tokens[insn.start..insn.end]);
            let mut operands = insn.operands();
            let dest = operands.next().ok_or(DxbcError::UnexpectedEndOfTokens)??;
            let src = operands.next().ok_or(DxbcError::UnexpectedEndOfTokens)??;
            out[base + (dest.token_offset - insn.start)] |= MASK_BIT_Z;
            let src_at = base + (src.token_offset - insn.start);
            out[src_at] = (out[src_at] & !SWIZZLE_COMPONENT_2_MASK) | SWIZZLE_Z_COMPONENT_2;
            continue;
        }
        out.extend_from_slice(&tokens[insn.start..insn.end]);
    }

    let length = out.len() as u32;
    out[1] = length;
    Ok(out.iter().flat_map(|t| t.to_le_bytes()).collect())
}

/// Rewrites the terrain tessellation **domain** shader for single-pass stereo by reprojection. The DS
/// builds clip in its own registers from `cb1`'s `m_OffsetViewProjection` (byte-identical to `cb0`'s
/// view-projection) and writes it to `SV_Position`; this renames those writes to a temp `rClip` and
/// post-multiplies by the per-eye `M_eye` before `ret`, exactly like [`reproject_vertex_shader`]. The
/// eye is not an `SV_InstanceID` here -- it rides in on the `TEXCOORD3.z` control-point lane the VS
/// originated ([`inject_eye_forward_vertex_shader`]) and the hull shader forwarded -- so the prologue
/// reads it from `vicp[0][2].z` (control point 0; the whole patch is one instance, hence one eye),
/// widens the `vicp[..][2]` input to `.xyz`, and writes `SV_ViewportArrayIndex` (legal from the last
/// pre-rasterization stage under the `SFI0` capability). `M_eye` and the `cb13` layout are shared with
/// the vertex reprojection.
pub fn reproject_domain_shader(blob: &[u8]) -> Result<Vec<u8>, DxbcError> {
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
    if stream.stage() != ShaderStage::Domain {
        return Err(DxbcError::NotDomainShader);
    }

    // The eye rides TEXCOORD3.z: confirm the input lane exists and its .z is free (mask is .xy).
    let (lane_register, lane_mask) =
        signature_register(isgn.body(blob), EYE_LANE_SEMANTIC, EYE_LANE_SEMANTIC_INDEX)
            .ok_or(DxbcError::NoEyeLane)?;
    if lane_mask & 0x4 != 0 {
        return Err(DxbcError::NoEyeLane);
    }

    // The free output register for the viewport must come from the output signature, not the body's
    // `dcl_output` opcodes: a shader can declare an output in its `OSGN` (occupying a register) without
    // ever writing it, so a `dcl_output` scan alone would under-count and collide with that dead slot.
    let output_register = max_signature_register(osgn.body(blob)).map_or(0, |r| r + 1);
    let plan = plan_domain_reproject(&stream, lane_register, output_register)?;
    let new_shex = rewrite_domain_reproject(&stream, &plan)?;

    // Widen the DS's TEXCOORD3 input lane to .xyz (both the component mask and the used mask), so the
    // .z carrying the eye is read.
    let new_isgn = widen_signature_mask(
        isgn.body(blob),
        EYE_LANE_SEMANTIC,
        EYE_LANE_SEMANTIC_INDEX,
        lane_mask | 0x4,
        lane_mask | 0x4,
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

/// What the terrain-DS reprojection scan learned.
struct DomainReprojectPlan {
    /// The output register `SV_Position` is declared at (`dcl_output_siv oN, position`).
    pos_register: u32,
    /// The `TEXCOORD3` control-point input register the eye rides in (`vicp[..][lane_register].z`).
    lane_register: u32,
    /// The `o` register for the new `SV_ViewportArrayIndex` output.
    output_register: u32,
    /// The first of the two temps this rewrite claims: `rBase` (eye row base) at `temp_base`, `rClip`
    /// (saved clip position) at `temp_base + 1`.
    temp_base: u32,
    /// The token index of the existing `dcl_temps`, if any (its count is bumped by two).
    dcl_temps_start: Option<usize>,
    /// The token index of the `dcl_input vicp[..][lane_register]` declaration, whose mask is widened
    /// to `.xyz`.
    lane_dcl_start: usize,
    /// The token index of the first non-declaration instruction, where declarations and the prologue
    /// are injected.
    inject_before: usize,
}

/// Scans the domain shader's declarations for the reprojection rewrite: the `SV_Position` output, the
/// free output register, the control-point lane declaration to widen, the temp count and injection
/// point, and the preconditions (`cb13` unclaimed, an `SV_Position` output present, no `retc`).
fn plan_domain_reproject(
    stream: &TokenStream,
    lane_register: u32,
    output_register: u32,
) -> Result<DomainReprojectPlan, DxbcError> {
    let tokens = stream.tokens();
    let mut temps: u32 = 0;
    let mut dcl_temps_start = None;
    let mut inject_before = None;
    let mut pos_register = None;
    let mut lane_dcl_start = None;

    for insn in stream.instructions() {
        let insn = insn?;
        if insn.is_declaration() {
            match insn.opcode {
                OPCODE_DCL_INPUT
                    if two_d_input_register(tokens, &insn, OPERAND_TYPE_INPUT_CONTROL_POINT)
                        == Some(lane_register) =>
                {
                    lane_dcl_start = Some(insn.start);
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
            if insn.opcode == OPCODE_RETC {
                return Err(DxbcError::UnsupportedControlFlow);
            }
        }
    }

    Ok(DomainReprojectPlan {
        pos_register: pos_register.ok_or(DxbcError::NoPositionOutput)?,
        lane_register,
        output_register,
        temp_base: temps,
        dcl_temps_start,
        lane_dcl_start: lane_dcl_start.ok_or(DxbcError::NoEyeLane)?,
        inject_before: inject_before.unwrap_or(tokens.len()),
    })
}

/// Rebuilds the domain shader chunk: injects the declarations and eye prologue, widens the lane
/// `dcl_input` to `.xyz`, renames every `SV_Position` write to `rClip`, and emits the `M_eye · rClip`
/// `dp4` chain before each `ret`.
fn rewrite_domain_reproject(
    stream: &TokenStream,
    plan: &DomainReprojectPlan,
) -> Result<Vec<u8>, DxbcError> {
    let tokens = stream.tokens();
    let mut out: Vec<u32> = Vec::with_capacity(tokens.len() + 128);
    out.extend_from_slice(&tokens[..2]);

    let mut injected = false;
    for insn in stream.instructions() {
        let insn = insn?;
        if insn.start == plan.inject_before {
            emit_domain_reproject_injection(&mut out, plan);
            injected = true;
        }
        if Some(insn.start) == plan.dcl_temps_start {
            out.push(tokens[insn.start]);
            out.push(plan.temp_base + 2);
            continue;
        }
        if insn.start == plan.lane_dcl_start {
            // Widen the lane input declaration `.xy` -> `.xyz` by setting the .z mask bit on its
            // operand token (the token right after the opcode).
            out.push(tokens[insn.start]);
            out.push(tokens[insn.start + 1] | MASK_BIT_Z);
            out.extend_from_slice(&tokens[insn.start + 2..insn.end]);
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
        emit_domain_reproject_injection(&mut out, plan);
    }

    let length = out.len() as u32;
    out[1] = length;
    Ok(out.iter().flat_map(|t| t.to_le_bytes()).collect())
}

/// Emits the domain-reprojection declarations and eye prologue: `cb13[18]`, the `SV_ViewportArrayIndex`
/// output, then `and rBase.x, vicp[0][lane].z, l(1)` (eye from the control-point lane), `mov oM.x,
/// rBase.x` (viewport), `imul null, rBase.x, rBase.x, l(4)` (the eye's `M_eye` row base). No
/// `SV_InstanceID` input -- the eye arrives on the lane, not as a system value.
fn emit_domain_reproject_injection(out: &mut Vec<u32>, plan: &DomainReprojectPlan) {
    let m = plan.output_register;
    let r = plan.temp_base;
    out.extend_from_slice(&[
        (4 << 24) | CB_ACCESS_DYNAMIC_INDEXED | OPCODE_DCL_CONSTANT_BUFFER,
        OPERAND_CB_2D_IMM,
        STEREO_CB_REGISTER,
        STEREO_REPROJ_CB_ROWS,
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
    // and rBase.x, vicp[0][lane].z, l(1) -- the eye index from the control-point lane. Eight dwords:
    // the `vicp[0][lane]` source is a 2D-indexed operand (token + two index tokens), one wider than
    // the vertex path's `vN.x`.
    out.extend_from_slice(&[
        (8 << 24) | OPCODE_AND,
        OPERAND_TEMP_MASK_X,
        r,
        OPERAND_VICP_SELECT_Z,
        0, // control point 0
        plan.lane_register,
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

/// The register index (second dimension) of a two-dimensionally-indexed `dcl_input` of the given
/// operand type, or `None` if the declaration is not such an input. The domain shader's control-point
/// inputs are `vicp[cp][reg]` ([`OPERAND_TYPE_INPUT_CONTROL_POINT`]); the hull control-point phase's
/// are plain `v[cp][reg]` ([`OPERAND_TYPE_INPUT`]). Both encode the register as the second index, so
/// this locates the `TEXCOORD3` lane declaration to widen in either stage.
fn two_d_input_register(tokens: &[u32], insn: &Instruction<'_>, operand_type: u32) -> Option<u32> {
    let tok = *tokens.get(insn.start + 1)?;
    if (tok >> 12) & 0xFF != operand_type || (tok >> 20) & 0x3 != 2 {
        return None;
    }
    tokens.get(insn.end - 1).copied()
}

/// What the terrain-VS eye-inject scan learned.
struct EyeInjectPlan {
    /// The `TEXCOORD3` output register the eye rides in (`.z`).
    lane_register: u32,
    /// The `v` register for the new `SV_InstanceID` input.
    input_register: u32,
    /// The token index of the lane's `dcl_output`, whose mask is widened to `.xyz`.
    dcl_output_lane_start: usize,
    /// The token index of the first non-declaration instruction -- where the new declaration and the
    /// eye write are injected.
    inject_before: usize,
}

/// Scans the terrain VS declarations for the eye-inject: the lane's `dcl_output`, the injection
/// point, and the precondition that `SV_InstanceID` is unclaimed. The free input register is supplied
/// by the caller from the input signature, which is authoritative over the declarations.
fn plan_eye_inject(
    stream: &TokenStream,
    lane_register: u32,
    input_register: u32,
) -> Result<EyeInjectPlan, DxbcError> {
    let tokens = stream.tokens();
    let mut dcl_output_lane_start = None;
    let mut inject_before = None;

    for insn in stream.instructions() {
        let insn = insn?;
        if insn.is_declaration() {
            match insn.opcode {
                OPCODE_DCL_INPUT_SGV if tokens[insn.end - 1] == SB_NAME_INSTANCE_ID => {
                    return Err(DxbcError::InstanceIdAlreadyDeclared);
                }
                OPCODE_DCL_OUTPUT | OPCODE_DCL_OUTPUT_SGV | OPCODE_DCL_OUTPUT_SIV
                    if declared_register(tokens, &insn)? == lane_register =>
                {
                    dcl_output_lane_start = Some(insn.start);
                }
                _ => {}
            }
        } else if inject_before.is_none() {
            inject_before = Some(insn.start);
        }
    }

    Ok(EyeInjectPlan {
        lane_register,
        input_register,
        dcl_output_lane_start: dcl_output_lane_start.ok_or(DxbcError::NoEyeLane)?,
        inject_before: inject_before.unwrap_or(tokens.len()),
    })
}

/// Rebuilds the terrain VS chunk: widens the lane `dcl_output` to `.xyz`, and injects the
/// `SV_InstanceID` declaration plus `and o<lane>.z, vN.x, l(1)` at the declaration boundary.
fn rewrite_eye_inject(stream: &TokenStream, plan: &EyeInjectPlan) -> Result<Vec<u8>, DxbcError> {
    let tokens = stream.tokens();
    let mut out: Vec<u32> = Vec::with_capacity(tokens.len() + 16);
    out.extend_from_slice(&tokens[..2]);

    let mut injected = false;
    for insn in stream.instructions() {
        let insn = insn?;
        if insn.start == plan.inject_before {
            emit_eye_inject(&mut out, plan.input_register, plan.lane_register);
            injected = true;
        }
        if insn.start == plan.dcl_output_lane_start {
            // Widen the lane output declaration `.xy` -> `.xyz` by setting the .z mask bit on its
            // operand token (the token right after the opcode).
            out.push(tokens[insn.start]);
            out.push(tokens[insn.start + 1] | MASK_BIT_Z);
            out.extend_from_slice(&tokens[insn.start + 2..insn.end]);
            continue;
        }
        out.extend_from_slice(&tokens[insn.start..insn.end]);
    }
    if !injected {
        emit_eye_inject(&mut out, plan.input_register, plan.lane_register);
    }

    let length = out.len() as u32;
    out[1] = length;
    Ok(out.iter().flat_map(|t| t.to_le_bytes()).collect())
}

/// Emits the `SV_InstanceID` declaration and the eye write `and o<lane>.z, vN.x, l(1)`.
fn emit_eye_inject(out: &mut Vec<u32>, input_register: u32, lane_register: u32) {
    out.extend_from_slice(&[
        (4 << 24) | OPCODE_DCL_INPUT_SGV,
        OPERAND_INPUT_MASK_X,
        input_register,
        SB_NAME_INSTANCE_ID,
    ]);
    out.extend_from_slice(&[
        (7 << 24) | OPCODE_AND,
        OPERAND_OUTPUT_MASK_Z,
        lane_register,
        OPERAND_INPUT_SELECT_X,
        input_register,
        OPERAND_IMM32_SCALAR,
        1,
    ]);
}
