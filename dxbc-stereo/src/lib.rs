//! DXBC bytecode surgery for single-pass stereo.
//!
//! Rewrites an opaque model vertex shader so one instanced draw renders both eyes: the per-eye data
//! in the position path is five `cb0` rows -- the camera position (`cb0[4]`) and the view-projection
//! (`cb0[29..32]`) -- so the transform binds a mod-owned `cb13` holding both eyes' five rows
//! (`[eye0: 0..4][eye1: 5..9]`) and rewrites the shader to index it per eye by `SV_InstanceID & 1`,
//! emitting `SV_ViewportArrayIndex`. See `docs/mod/single-pass-stereo.md`.
//!
//! Two layers: the *analysis* foundation (parse the DXBC container and the SM5 token stream, and
//! locate the operands the rewrite must remap -- [`per_eye_refs`]) and the *rewrite* built on it
//! ([`patch_vertex_shader`]). Kept pure and portable so it unit-tests natively against the game's
//! shaders; the payload runs it in-flight in the `CreateVertexProgram` hook.

mod checksum;
mod container;
mod rewrite;
mod tokens;

pub use checksum::refresh_checksum;
pub use container::{Chunk, Dxbc, DxbcError};
pub use rewrite::{
    MEYE_ROW_BASE, STEREO_CB_REGISTER, STEREO_CB_ROWS, STEREO_REPROJ_CB_ROWS,
    forward_eye_hull_shader, inject_eye_forward_vertex_shader, patch_vertex_shader,
    reproject_domain_shader, reproject_vertex_shader,
};
pub use tokens::{Operand, OperandKind, ShaderStage, TokenStream};

/// The `cb0` rows that carry per-eye data in the vertex position path: `cb0[4]` (camera world
/// position) and `cb0[29..32]` (the view-projection). These are the operands the single-pass
/// rewrite remaps to the per-eye `cb13`.
pub const PER_EYE_CB0_ROWS: [u32; 5] = [4, 29, 30, 31, 32];

/// A reference to a per-eye `cb0` operand found in a shader: the `cb0` row and the token index of
/// the operand within the shader chunk (for the rewrite to target).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerEyeRef {
    /// The `cb0` element index (one of [`PER_EYE_CB0_ROWS`]).
    pub row: u32,
    /// The token offset of the operand within the shader chunk's token stream.
    pub token_offset: usize,
}

/// Finds every `cb0[{4,29..32}]` operand in a vertex shader's token stream -- the per-eye view rows
/// the single-pass rewrite retargets to `cb13`. Returns them in program order.
pub fn per_eye_refs(blob: &[u8]) -> Result<Vec<PerEyeRef>, DxbcError> {
    let dxbc = Dxbc::parse(blob)?;
    let shex = dxbc.shader_chunk().ok_or(DxbcError::NoShaderChunk)?;
    let stream = TokenStream::new(shex.body(blob))?;

    let mut refs = Vec::new();
    for insn in stream.instructions() {
        let insn = insn?;
        // A declaration's cb operand encodes the buffer size, not a row access -- skip it so a buffer
        // declared at a per-eye size (`dcl_constantbuffer cb0[29]`) is not counted as a reference.
        if insn.is_declaration() {
            continue;
        }
        for operand in insn.operands() {
            let operand = operand?;
            if let OperandKind::ConstantBuffer {
                register: 0,
                element,
            } = operand.kind
                && PER_EYE_CB0_ROWS.contains(&element)
            {
                refs.push(PerEyeRef {
                    row: element,
                    token_offset: operand.token_offset,
                });
            }
        }
    }
    Ok(refs)
}
