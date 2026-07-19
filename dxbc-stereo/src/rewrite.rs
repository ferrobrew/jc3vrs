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
/// The `cb13` row where the reprojection `M_eye` block begins, after the 10 `cb0`-remap rows. Eye
/// `e`'s `M_eye` is the four rows `cb13[MEYE_ROW_BASE + 4*e .. +4]`, addressed as
/// `cb13[rBase.x + (MEYE_ROW_BASE + j)]` with `rBase.x = 4*e`.
pub const MEYE_ROW_BASE: u32 = STEREO_CB_ROWS;
/// The `cb13` size a reprojected shader declares: the 10 `cb0`-remap rows plus a four-rows-per-eye
/// `M_eye` block. The `cb0`-remap shaders declare only `CB13[10]`; both idioms bind the same `b13`
/// buffer, which the payload sizes to this full length.
pub const STEREO_REPROJ_CB_ROWS: u32 = STEREO_CB_ROWS + 8;

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

    Ok(reassemble(&dxbc, blob, new_isgn, new_osgn, new_shex, true))
}

/// Reassembles a container with the rewritten `ISGN`/`OSGN`/`SHEX` chunks swapped in, the `SFI0`
/// viewport-feature bit set (adding an `SFI0` chunk after `SHEX` if the container had none -- fxc's
/// chunk order is `RDEF, ISGN, OSGN, SHEX, SFI0, STAT`), and a refreshed checksum. Shared by the
/// `cb0`-remap and reprojection rewrites, which differ only in how they build the new chunks.
fn reassemble(
    dxbc: &Dxbc,
    blob: &[u8],
    new_isgn: Vec<u8>,
    new_osgn: Vec<u8>,
    new_shex: Vec<u8>,
    add_viewport_sfi0: bool,
) -> Vec<u8> {
    let mut chunks: Vec<([u8; 4], Vec<u8>)> = Vec::with_capacity(dxbc.chunks().len() + 1);
    let has_sfi0 = dxbc.chunk(b"SFI0").is_some();
    for chunk in dxbc.chunks() {
        let body = match &chunk.tag {
            b"ISGN" => new_isgn.clone(),
            b"OSGN" => new_osgn.clone(),
            b"SHEX" => new_shex.clone(),
            b"SFI0" if add_viewport_sfi0 => with_viewport_feature_bit(chunk.body(blob)),
            _ => chunk.body(blob).to_vec(),
        };
        chunks.push((chunk.tag, body));
        if &chunk.tag == b"SHEX" && add_viewport_sfi0 && !has_sfi0 {
            chunks.push((*b"SFI0", with_viewport_feature_bit(&[])));
        }
    }

    let mut out = build_container(&chunks);
    refresh_checksum(&mut out);
    out
}

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

    let plan = plan_reproject(&stream)?;
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

    Ok(reassemble(&dxbc, blob, new_isgn, new_osgn, new_shex, true))
}

/// The output semantic that carries the single-pass eye index through the terrain tessellation
/// pipeline: `TEXCOORD3`'s free `.z` component (the VS uses only `.xy`). The VS writes the eye there,
/// the hull shader forwards it, and the domain shader reads it to reproject and route.
const EYE_LANE_SEMANTIC: &str = "TEXCOORD";
const EYE_LANE_SEMANTIC_INDEX: u32 = 3;
/// The `.z` write-mask bit in an operand token's mask nibble (bit 6 of the low byte). ORing it into a
/// `dcl_output`/`mov` operand widens `.xy` to `.xyz`.
const MASK_BIT_Z: u32 = 0x40;

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

    let plan = plan_eye_inject(&stream, lane_register)?;
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

    Ok(reassemble(&dxbc, blob, new_isgn, new_osgn, new_shex, false))
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

/// Scans the terrain VS declarations for the eye-inject: the free input register, the lane's
/// `dcl_output`, the injection point, and the precondition that `SV_InstanceID` is unclaimed.
fn plan_eye_inject(stream: &TokenStream, lane_register: u32) -> Result<EyeInjectPlan, DxbcError> {
    let tokens = stream.tokens();
    let mut max_input: Option<u32> = None;
    let mut dcl_output_lane_start = None;
    let mut inject_before = None;

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
        input_register: max_input.map_or(0, |r| r + 1),
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
// Reprojection opcodes.
const OPCODE_DP4: u32 = 0x11;
const OPCODE_RET: u32 = 0x3E;
const OPCODE_RETC: u32 = 0x25;

/// SM4/5 operand type: output register (`oN`). Reprojection renames `SV_Position` writes (which are
/// output-register destinations) to a temp by clearing this type field to `0` (TEMP).
const OPERAND_TYPE_OUTPUT: u32 = 2;
/// `D3D_NAME_POSITION`, the `dcl_output_siv` trailing system-value name for `SV_Position`.
const SB_NAME_POSITION: u32 = 1;

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
/// `oM.y`/`.z`/`.w` as a destination: [`OPERAND_OUTPUT_MASK_X`] with the write-mask nibble moved to
/// the y/z/w component. The reprojection epilogue writes one clip component per `dp4`.
const OPERAND_OUTPUT_MASK_Y: u32 = 0x0010_2022;
const OPERAND_OUTPUT_MASK_Z: u32 = 0x0010_2042;
const OPERAND_OUTPUT_MASK_W: u32 = 0x0010_2082;
/// `rN.xyzw` as a source: temp register, 1D immediate index, full `.xyzw` swizzle (the saved clip
/// position each `M_eye` row dots against).
const OPERAND_TEMP_SWIZZLE_XYZW: u32 = 0x0010_0E46;
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

/// Scans the declarations for the reprojection rewrite: the `SV_Position` output register, the free
/// input/output registers, the temp count and injection point, and the preconditions (`cb13` and
/// `SV_InstanceID` unclaimed, an `SV_Position` output present, no `retc`).
fn plan_reproject(stream: &TokenStream) -> Result<ReprojectPlan, DxbcError> {
    let tokens = stream.tokens();
    let mut max_input: Option<u32> = None;
    let mut max_output: Option<u32> = None;
    let mut temps: u32 = 0;
    let mut dcl_temps_start = None;
    let mut inject_before = None;
    let mut pos_register = None;

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
                    if insn.opcode == OPCODE_DCL_OUTPUT_SIV
                        && tokens[insn.end - 1] == SB_NAME_POSITION
                    {
                        pos_register = Some(register);
                    }
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
        input_register: max_input.map_or(0, |r| r + 1),
        output_register: max_output.map_or(0, |r| r + 1),
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
            emit_meye_epilogue(&mut out, plan);
            out.extend_from_slice(&tokens[insn.start..insn.end]);
            continue;
        }
        rewrite_reproject_instruction(tokens, &insn, plan, &mut out)?;
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

/// Emits `o<pos> = M_eye · rClip` as four `dp4`s, one clip component each: `dp4 o<pos>.{x,y,z,w},
/// cb13[rBase.x + (MEYE_ROW_BASE + j)], rClip.xyzw`. `rBase.x` holds `4*eye`, so the immediate row
/// index selects the eye's `M_eye` block relative to it.
fn emit_meye_epilogue(out: &mut Vec<u32>, plan: &ReprojectPlan) {
    let pos = plan.pos_register;
    let rbase = plan.temp_base;
    let rclip = plan.temp_base + 1;
    const OUT_MASKS: [u32; 4] = [
        OPERAND_OUTPUT_MASK_X,
        OPERAND_OUTPUT_MASK_Y,
        OPERAND_OUTPUT_MASK_Z,
        OPERAND_OUTPUT_MASK_W,
    ];
    for (j, &mask) in OUT_MASKS.iter().enumerate() {
        out.extend_from_slice(&[
            (10 << 24) | OPCODE_DP4,
            mask,
            pos,
            OPERAND_CB_2D_IMM | (IDX_IMM32_PLUS_RELATIVE << 25),
            STEREO_CB_REGISTER,
            MEYE_ROW_BASE + j as u32,
            OPERAND_TEMP_SELECT_X,
            rbase,
            OPERAND_TEMP_SWIZZLE_XYZW,
            rclip,
        ]);
    }
}

/// Re-serializes one instruction for reprojection, renaming any write to the `SV_Position` output
/// register into the `rClip` temp (operand type OUTPUT -> TEMP, register -> `rClip`). Output
/// registers are write-only, so every occurrence is a write and the rename leaves `rClip` holding
/// the shader's own clip position for the epilogue.
fn rewrite_reproject_instruction(
    tokens: &[u32],
    insn: &Instruction<'_>,
    plan: &ReprojectPlan,
    out: &mut Vec<u32>,
) -> Result<(), DxbcError> {
    let rclip = plan.temp_base + 1;
    out.extend_from_slice(&tokens[insn.start..insn.operands_start()]);

    let mut pos = insn.operands_start();
    while pos < insn.end {
        let (_, next) = parse_operand(tokens, pos)?;
        let tok = tokens[pos];
        let operand_type = (tok >> 12) & 0xFF;
        let index_dim = (tok >> 20) & 0x3;
        let extended = ((tok >> 31) & 1) as usize;
        let rep = (tok >> 22) & 0x7;
        let reg_at = pos + 1 + extended;
        let is_position_write = operand_type == OPERAND_TYPE_OUTPUT
            && index_dim == 1
            && rep == 0 // IDX_IMM32
            && tokens.get(reg_at) == Some(&plan.pos_register);
        if is_position_write {
            // OUTPUT (2) -> TEMP (0): clear the operand-type field, keep the mask/swizzle; swap the
            // register index to rClip. Token count is unchanged, so the instruction length stands.
            out.push(tok & !(0xFF << 12));
            out.extend_from_slice(&tokens[pos + 1..reg_at]);
            out.push(rclip);
            out.extend_from_slice(&tokens[reg_at + 1..next]);
        } else {
            out.extend_from_slice(&tokens[pos..next]);
        }
        pos = next;
    }
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

/// The register and component mask of the `ISGN`/`OSGN` element with the given semantic name and
/// index, or `None` if absent. Used by the terrain eye-lane rewrites to locate the `TEXCOORD3` lane.
fn signature_register(body: &[u8], name: &str, semantic_index: u32) -> Option<(u32, u8)> {
    if body.len() < 8 {
        return None;
    }
    let count = u32::from_le_bytes(body[0..4].try_into().ok()?) as usize;
    let table = u32::from_le_bytes(body[4..8].try_into().ok()?) as usize;
    for i in 0..count {
        let rec = table.checked_add(i.checked_mul(SIGNATURE_ELEMENT_LEN)?)?;
        let record = body.get(rec..rec + SIGNATURE_ELEMENT_LEN)?;
        let name_off = u32::from_le_bytes(record[0..4].try_into().ok()?) as usize;
        let sem_idx = u32::from_le_bytes(record[4..8].try_into().ok()?);
        let reg = u32::from_le_bytes(record[16..20].try_into().ok()?);
        let elem_name = body.get(name_off..)?.split(|&b| b == 0).next()?;
        if elem_name == name.as_bytes() && sem_idx == semantic_index {
            return Some((reg, record[20]));
        }
    }
    None
}

/// Sets the component mask and rw-mask of the `ISGN`/`OSGN` element with the given semantic name and
/// index, in place. The record size is unchanged, so no name offset shifts -- unlike
/// [`append_signature_element`]. Used to widen the `TEXCOORD3` lane's mask to `.xyz`.
fn widen_signature_mask(
    body: &[u8],
    name: &str,
    semantic_index: u32,
    mask: u8,
    rw_mask: u8,
) -> Result<Vec<u8>, DxbcError> {
    if body.len() < 8 {
        return Err(DxbcError::MalformedSignature);
    }
    let count = u32::from_le_bytes(body[0..4].try_into().expect("4 bytes")) as usize;
    let table = u32::from_le_bytes(body[4..8].try_into().expect("4 bytes")) as usize;
    let mut out = body.to_vec();
    for i in 0..count {
        let rec = table + i * SIGNATURE_ELEMENT_LEN;
        if rec + SIGNATURE_ELEMENT_LEN > body.len() {
            return Err(DxbcError::MalformedSignature);
        }
        let name_off = u32::from_le_bytes(body[rec..rec + 4].try_into().expect("4 bytes")) as usize;
        let sem_idx = u32::from_le_bytes(body[rec + 4..rec + 8].try_into().expect("4 bytes"));
        let elem_name = body
            .get(name_off..)
            .and_then(|s| s.split(|&b| b == 0).next())
            .unwrap_or(&[]);
        if elem_name == name.as_bytes() && sem_idx == semantic_index {
            out[rec + 20] = mask;
            out[rec + 21] = rw_mask;
            return Ok(out);
        }
    }
    Err(DxbcError::MalformedSignature)
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
