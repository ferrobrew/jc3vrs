//! The SM5 token stream: walking instructions and decoding operands.
//!
//! The shader chunk is `[version u32][length-in-dwords u32][tokens...]`. Each instruction begins
//! with an opcode token whose bits `[30:24]` give its length in dwords (a couple of opcodes carry a
//! custom length); operand tokens encode an operand type at bits `[19:12]`, an index dimension at
//! `[21:20]`, and per-dimension index representations at `[24:22]`/`[27:25]`/`[30:28]`.

use crate::container::DxbcError;

/// The shader stage a DXBC chunk targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderStage {
    Pixel,
    Vertex,
    Geometry,
    Hull,
    Domain,
    Compute,
    Unknown(u32),
}

/// A shader's SM5 token stream (the `SHEX`/`SHDR` chunk body as dwords).
pub struct TokenStream {
    tokens: Vec<u32>,
}

impl TokenStream {
    /// Reads the version/length header and the token array from a shader chunk body.
    pub fn new(body: &[u8]) -> Result<TokenStream, DxbcError> {
        if body.len() < 8 {
            return Err(DxbcError::Truncated);
        }
        let length = u32::from_le_bytes([body[4], body[5], body[6], body[7]]) as usize;
        if body.len() < length * 4 {
            return Err(DxbcError::Truncated);
        }
        let tokens = (0..length)
            .map(|i| {
                let b = &body[i * 4..i * 4 + 4];
                u32::from_le_bytes([b[0], b[1], b[2], b[3]])
            })
            .collect();
        Ok(TokenStream { tokens })
    }

    /// The shader stage, from the version token's program-type field.
    pub fn stage(&self) -> ShaderStage {
        match self.tokens.first().copied().unwrap_or(0) >> 16 {
            0 => ShaderStage::Pixel,
            1 => ShaderStage::Vertex,
            2 => ShaderStage::Geometry,
            3 => ShaderStage::Hull,
            4 => ShaderStage::Domain,
            5 => ShaderStage::Compute,
            other => ShaderStage::Unknown(other),
        }
    }

    /// Iterates the instructions in program order.
    pub fn instructions(&self) -> Instructions<'_> {
        Instructions {
            tokens: &self.tokens,
            pos: 2, // skip version + length
        }
    }

    /// The raw dword tokens, including the version and length header. The rewrite reads these to
    /// re-serialize instructions it edits.
    pub(crate) fn tokens(&self) -> &[u32] {
        &self.tokens
    }
}

/// The opcode field of an opcode token (`[10:0]`).
const OPCODE_MASK: u32 = 0x7FF;
/// The custom-data opcode, whose length lives in the *next* dword rather than the opcode token.
const OPCODE_CUSTOMDATA: u32 = 0x35;

/// An iterator over a token stream's instructions.
pub struct Instructions<'a> {
    tokens: &'a [u32],
    pos: usize,
}

impl<'a> Iterator for Instructions<'a> {
    type Item = Result<Instruction<'a>, DxbcError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.tokens.len() {
            return None;
        }
        let start = self.pos;
        let op0 = self.tokens[start];
        let opcode = op0 & OPCODE_MASK;

        if opcode == OPCODE_CUSTOMDATA {
            let Some(&len) = self.tokens.get(start + 1) else {
                return Some(Err(DxbcError::UnexpectedEndOfTokens));
            };
            let len = len as usize;
            if len == 0 || start + len > self.tokens.len() {
                return Some(Err(DxbcError::ZeroLengthInstruction));
            }
            self.pos = start + len;
            return Some(Ok(Instruction {
                tokens: self.tokens,
                opcode,
                start,
                end: self.pos,
                operands_start: self.pos, // custom data has no operands to decode
            }));
        }

        let len = ((op0 >> 24) & 0x7F) as usize;
        if len == 0 {
            return Some(Err(DxbcError::ZeroLengthInstruction));
        }
        let end = start + len;
        if end > self.tokens.len() {
            return Some(Err(DxbcError::UnexpectedEndOfTokens));
        }

        // Skip any chained extended opcode tokens to reach the first operand.
        let mut p = start + 1;
        let mut ext = (op0 >> 31) & 1;
        while ext == 1 {
            let Some(&tok) = self.tokens.get(p) else {
                return Some(Err(DxbcError::UnexpectedEndOfTokens));
            };
            p += 1;
            ext = (tok >> 31) & 1;
        }

        self.pos = end;
        Some(Ok(Instruction {
            tokens: self.tokens,
            opcode,
            start,
            end,
            operands_start: p,
        }))
    }
}

/// One decoded instruction, with a view over its operand tokens.
pub struct Instruction<'a> {
    tokens: &'a [u32],
    /// The opcode field.
    pub opcode: u32,
    /// The token index of the opcode token.
    pub start: usize,
    /// The token index one past the instruction.
    pub end: usize,
    operands_start: usize,
}

impl<'a> Instruction<'a> {
    /// Iterates the instruction's operands.
    pub fn operands(&self) -> Operands<'a> {
        Operands {
            tokens: self.tokens,
            pos: self.operands_start,
            end: self.end,
        }
    }

    /// The token index of the first operand (past the opcode and any extended opcode tokens). The
    /// rewrite copies `start..operands_start` verbatim when re-serializing an instruction.
    pub(crate) fn operands_start(&self) -> usize {
        self.operands_start
    }

    /// Whether this is a declaration (`dcl_*`) or the custom-data block that rides among them, rather
    /// than an executable instruction. A declaration's constant-buffer operand encodes the buffer
    /// *size* (`dcl_constantbuffer cb0[29]`), not a row access, so per-eye analysis and the operand
    /// rewrite both skip declarations -- otherwise a buffer sized to a per-eye row (e.g. `cb0[29]`)
    /// reads as a phantom per-eye reference. It also fixes the rewrite's injection point (the first
    /// executable instruction), so it must recognise **every** declaration opcode: the SM4 block
    /// (`0x58..=0x6A`) *and* the SM5 block (`0x91..=0xA2`, `dcl_stream` through
    /// `dcl_resource_structured` -- the tessellation, UAV, and structured-resource declarations the
    /// terrain and vegetation shaders use; `dcl_resource_structured` is `0xA2`, with the executable
    /// `store_*`/`ld_structured` opcodes at `0xA3+`). Missing the SM5 block would treat
    /// `dcl_resource_structured` as the first instruction and inject the prologue mid-declarations.
    pub fn is_declaration(&self) -> bool {
        matches!(self.opcode, OPCODE_CUSTOMDATA | 0x58..=0x6A | 0x91..=0xA2)
    }
}

/// An iterator over an instruction's operands.
pub struct Operands<'a> {
    tokens: &'a [u32],
    pos: usize,
    end: usize,
}

impl Iterator for Operands<'_> {
    type Item = Result<Operand, DxbcError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.end {
            return None;
        }
        match parse_operand(self.tokens, self.pos) {
            Ok((operand, next)) => {
                if next <= self.pos {
                    return Some(Err(DxbcError::UnexpectedEndOfTokens));
                }
                self.pos = next;
                Some(Ok(operand))
            }
            Err(e) => {
                self.pos = self.end; // stop iterating on error
                Some(Err(e))
            }
        }
    }
}

/// A decoded operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Operand {
    /// What the operand refers to.
    pub kind: OperandKind,
    /// The token index of the operand's opcode token.
    pub token_offset: usize,
}

/// The kinds of operand this analysis distinguishes (everything else is [`OperandKind::Other`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperandKind {
    /// A constant-buffer reference `cbN[element]` with immediate indices.
    ConstantBuffer { register: u32, element: u32 },
    /// A constant-buffer reference with a dynamic (relative) element index, e.g. `cb13[r0.x + k]`.
    ConstantBufferDynamic { register: u32 },
    /// An inline immediate value operand.
    Immediate,
    /// Any other operand type (registers, inputs, outputs, samplers, ...).
    Other,
}

/// D3D10/11 operand type: constant buffer.
const OPERAND_TYPE_CONSTANT_BUFFER: u32 = 8;
/// D3D10/11 operand types: inline immediates.
const OPERAND_TYPE_IMMEDIATE32: u32 = 4;
const OPERAND_TYPE_IMMEDIATE64: u32 = 5;

// Index representations (per dimension).
const IDX_IMM32: u32 = 0;
const IDX_IMM64: u32 = 1;
const IDX_RELATIVE: u32 = 2;
pub(crate) const IDX_IMM32_PLUS_RELATIVE: u32 = 3;
const IDX_IMM64_PLUS_RELATIVE: u32 = 4;

/// Parses one operand starting at `tokens[pos]`; returns the decoded operand and the token index
/// just past it. Recurses into relative-index operands.
pub(crate) fn parse_operand(tokens: &[u32], pos: usize) -> Result<(Operand, usize), DxbcError> {
    let tok = *tokens.get(pos).ok_or(DxbcError::UnexpectedEndOfTokens)?;
    let token_offset = pos;
    let mut p = pos + 1;

    let num_components_enum = tok & 0x3;
    let operand_type = (tok >> 12) & 0xFF;
    let index_dim = (tok >> 20) & 0x3;
    let extended = (tok >> 31) & 1;
    if extended == 1 {
        p += 1; // skip the extended operand token (modifiers); not needed here
    }

    // Immediate operands carry their values inline instead of indices.
    if operand_type == OPERAND_TYPE_IMMEDIATE32 {
        let count = immediate_component_count(num_components_enum);
        return end_operand(
            OperandKind::Immediate,
            token_offset,
            p + count,
            tokens.len(),
        );
    }
    if operand_type == OPERAND_TYPE_IMMEDIATE64 {
        let count = immediate_component_count(num_components_enum);
        return end_operand(
            OperandKind::Immediate,
            token_offset,
            p + count * 2,
            tokens.len(),
        );
    }

    let mut register: Option<u32> = None;
    let mut element_imm: Option<u32> = None;
    let mut element_dynamic = false;

    for d in 0..index_dim {
        let rep = (tok >> (22 + 3 * d)) & 0x7;
        let mut imm: Option<u32> = None;
        match rep {
            IDX_IMM32 | IDX_IMM32_PLUS_RELATIVE => {
                imm = Some(*tokens.get(p).ok_or(DxbcError::UnexpectedEndOfTokens)?);
                p += 1;
            }
            IDX_IMM64 | IDX_IMM64_PLUS_RELATIVE => {
                // Low dword is the value we care about; skip both.
                imm = Some(*tokens.get(p).ok_or(DxbcError::UnexpectedEndOfTokens)?);
                p += 2;
            }
            _ => {}
        }
        let is_relative = matches!(
            rep,
            IDX_RELATIVE | IDX_IMM32_PLUS_RELATIVE | IDX_IMM64_PLUS_RELATIVE
        );
        if is_relative {
            // The relative index is itself an operand; recurse to skip it.
            let (_, next) = parse_operand(tokens, p)?;
            p = next;
        }
        if d == 0 {
            register = imm;
        } else if d == 1 {
            element_imm = imm;
            element_dynamic = is_relative;
        }
    }

    let kind = if operand_type == OPERAND_TYPE_CONSTANT_BUFFER {
        match (register, element_imm, element_dynamic) {
            (Some(register), Some(element), false) => {
                OperandKind::ConstantBuffer { register, element }
            }
            (Some(register), _, true) => OperandKind::ConstantBufferDynamic { register },
            _ => OperandKind::Other,
        }
    } else {
        OperandKind::Other
    };

    end_operand(kind, token_offset, p, tokens.len())
}

/// Finalize a parsed operand, rejecting one whose span runs past the end of the token slice. Keeps
/// `parse_operand` total: it never returns a `next` index a caller could slice out of bounds (an
/// unusual or truncated operand encoding becomes an error rather than a panic downstream).
fn end_operand(
    kind: OperandKind,
    token_offset: usize,
    next: usize,
    len: usize,
) -> Result<(Operand, usize), DxbcError> {
    if next > len {
        return Err(DxbcError::UnexpectedEndOfTokens);
    }
    Ok((Operand { kind, token_offset }, next))
}

/// Dword count of an inline immediate operand's values, from its `D3D10_SB_OPERAND_NUM_COMPONENTS`
/// enum: `0` = zero-component (no values), `1` = one-component (the scalar `l(x)` form), `2` =
/// four-component (the common `l(x, y, z, w)` form). `fxc` does not emit the N-component form (`3`)
/// for immediates.
fn immediate_component_count(num_components_enum: u32) -> usize {
    match num_components_enum {
        0 => 0, // zero-component: no inline values
        1 => 1, // one-component: l(x)
        _ => 4, // four-component: l(x, y, z, w)
    }
}
