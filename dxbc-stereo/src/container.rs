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
}

impl fmt::Display for DxbcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            DxbcError::NotDxbc => "dxbc: not a DXBC container (bad magic)",
            DxbcError::Truncated => "dxbc: container is truncated",
            DxbcError::NoShaderChunk => "dxbc: no SHEX/SHDR shader chunk",
            DxbcError::ZeroLengthInstruction => "dxbc: zero-length instruction token",
            DxbcError::UnexpectedEndOfTokens => "dxbc: token stream ended mid-instruction",
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
