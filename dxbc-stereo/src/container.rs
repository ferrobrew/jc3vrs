//! The DXBC container: magic, checksum, and the chunk table.

use std::fmt;

/// A parsed DXBC container -- its chunk table indexed by four-character tag.
pub struct Dxbc {
    chunks: Vec<Chunk>,
}

/// One chunk in a DXBC container.
#[derive(Debug, Clone, Copy)]
pub struct Chunk {
    /// The four-character chunk tag (`SHEX`, `ISGN`, `OSG5`, ...).
    pub tag: [u8; 4],
    /// The byte offset of the chunk body (past the 8-byte tag+size header) within the container.
    pub body_offset: usize,
    /// The chunk body length in bytes.
    pub body_len: usize,
}

impl Chunk {
    /// The chunk body as a slice of the container.
    pub fn body<'a>(&self, blob: &'a [u8]) -> &'a [u8] {
        &blob[self.body_offset..self.body_offset + self.body_len]
    }
}

impl Dxbc {
    /// Parses a DXBC container's header and chunk table. Does not parse chunk contents.
    pub fn parse(blob: &[u8]) -> Result<Dxbc, DxbcError> {
        if blob.len() < 0x20 || &blob[..4] != b"DXBC" {
            return Err(DxbcError::NotDxbc);
        }
        let chunk_count = read_u32(blob, 0x1C)? as usize;
        let table_end = 0x20 + chunk_count * 4;
        if blob.len() < table_end {
            return Err(DxbcError::Truncated);
        }
        let mut chunks = Vec::with_capacity(chunk_count);
        for i in 0..chunk_count {
            let off = read_u32(blob, 0x20 + i * 4)? as usize;
            if off + 8 > blob.len() {
                return Err(DxbcError::Truncated);
            }
            let tag = [blob[off], blob[off + 1], blob[off + 2], blob[off + 3]];
            let body_len = read_u32(blob, off + 4)? as usize;
            let body_offset = off + 8;
            if body_offset + body_len > blob.len() {
                return Err(DxbcError::Truncated);
            }
            chunks.push(Chunk {
                tag,
                body_offset,
                body_len,
            });
        }
        Ok(Dxbc { chunks })
    }

    /// The chunks in container order.
    pub fn chunks(&self) -> &[Chunk] {
        &self.chunks
    }

    /// The chunk with the given tag, if present.
    pub fn chunk(&self, tag: &[u8; 4]) -> Option<Chunk> {
        self.chunks.iter().copied().find(|c| &c.tag == tag)
    }

    /// The shader bytecode chunk (`SHEX` for SM5, `SHDR` for older), if present.
    pub fn shader_chunk(&self) -> Option<Chunk> {
        self.chunk(b"SHEX").or_else(|| self.chunk(b"SHDR"))
    }
}

/// A DXBC parse error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DxbcError {
    /// The blob does not start with the `DXBC` magic.
    NotDxbc,
    /// The blob is shorter than its header/chunk table declare.
    Truncated,
    /// The container has no `SHEX`/`SHDR` shader chunk.
    NoShaderChunk,
    /// An instruction's declared length was zero, which would loop the token walk forever.
    ZeroLengthInstruction,
    /// The token stream ended inside an instruction or operand.
    UnexpectedEndOfTokens,
    /// The shader is not a vertex shader, so the stereo rewrite does not apply.
    NotVertexShader,
    /// The shader is SM4 (`SHDR`); the rewrite emits SM5-era structures (`SFI0`, the viewport
    /// output) and only supports `SHEX` shaders.
    UnsupportedShaderModel,
    /// The container has no `ISGN` input-signature chunk to extend.
    MissingInputSignature,
    /// The container has no `OSGN` output-signature chunk to extend.
    MissingOutputSignature,
    /// A signature chunk's header or element table does not match the expected `ISGN`/`OSGN`
    /// 24-byte-element layout.
    MalformedSignature,
    /// The shader has no `cb0` per-eye operands, so there is nothing to rewrite; the caller should
    /// leave it double-drawn.
    NoPerEyeReferences,
    /// The shader already declares `cb13`, the slot the stereo constants need.
    Cb13AlreadyDeclared,
    /// The shader already declares an `SV_InstanceID` input; rewriting its consumers is not
    /// supported yet.
    InstanceIdAlreadyDeclared,
    /// A per-eye operand uses an index encoding the rewrite does not support (for instance a 64-bit
    /// or already-relative element index).
    UnsupportedOperandEncoding,
    /// Rewriting an instruction would push its length past the 7-bit instruction-length field.
    InstructionTooLong,
    /// The reprojection rewrite found no `SV_Position` output to reproject (no `dcl_output_siv
    /// position`), so there is no clip position to post-multiply.
    NoPositionOutput,
    /// The reprojection rewrite hit a control-flow shape it does not support -- a conditional early
    /// return (`retc`) would need the per-eye epilogue on that path too.
    UnsupportedControlFlow,
    /// The terrain eye-lane rewrite found no free `TEXCOORD3.z` output lane to carry the eye index
    /// through the VS -> HS -> DS pipeline (the `TEXCOORD3` output is missing, or its `.z` is used).
    NoEyeLane,
}

impl fmt::Display for DxbcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            DxbcError::NotDxbc => "dxbc: not a DXBC container (bad magic)",
            DxbcError::Truncated => "dxbc: container is truncated",
            DxbcError::NoShaderChunk => "dxbc: no SHEX/SHDR shader chunk",
            DxbcError::ZeroLengthInstruction => "dxbc: zero-length instruction token",
            DxbcError::UnexpectedEndOfTokens => "dxbc: token stream ended mid-instruction",
            DxbcError::NotVertexShader => "dxbc: not a vertex shader",
            DxbcError::UnsupportedShaderModel => "dxbc: SM4 (SHDR) shader; the rewrite needs SHEX",
            DxbcError::MissingInputSignature => "dxbc: no ISGN input-signature chunk",
            DxbcError::MissingOutputSignature => "dxbc: no OSGN output-signature chunk",
            DxbcError::MalformedSignature => "dxbc: signature chunk has an unexpected layout",
            DxbcError::NoPerEyeReferences => "dxbc: no per-eye cb0 operands to rewrite",
            DxbcError::Cb13AlreadyDeclared => "dxbc: the shader already declares cb13",
            DxbcError::InstanceIdAlreadyDeclared => {
                "dxbc: the shader already declares an SV_InstanceID input"
            }
            DxbcError::UnsupportedOperandEncoding => {
                "dxbc: a per-eye operand uses an unsupported index encoding"
            }
            DxbcError::InstructionTooLong => {
                "dxbc: a rewritten instruction exceeds the instruction-length field"
            }
            DxbcError::NoPositionOutput => "dxbc: no SV_Position output to reproject",
            DxbcError::UnsupportedControlFlow => {
                "dxbc: reprojection does not support a conditional early return (retc)"
            }
            DxbcError::NoEyeLane => "dxbc: no free TEXCOORD3.z lane to carry the eye index",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for DxbcError {}

fn read_u32(blob: &[u8], off: usize) -> Result<u32, DxbcError> {
    blob.get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(DxbcError::Truncated)
}
