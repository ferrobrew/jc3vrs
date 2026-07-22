//! Shared DXBC scaffolding for the single-pass stereo rewrites.
//!
//! Holds the pieces two or more of the transform modules lean on: the `cb13` layout constants, the
//! container reassembly and checksum refresh, the `M_eye` reprojection epilogue and its
//! `SV_Position`-rename helper, the `ISGN`/`OSGN` signature surgery, and the SM4/5 token vocabulary
//! the injected code is spelled in. The terrain-only vocabulary lives in `terrain` instead.

use crate::{
    checksum::refresh_checksum,
    container::{Dxbc, DxbcError},
    tokens::{IDX_IMM32_PLUS_RELATIVE, Instruction, parse_operand},
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

/// Reassembles a container with the rewritten `ISGN`/`OSGN`/`SHEX` chunks swapped in, the `SFI0`
/// viewport-feature bit set (adding an `SFI0` chunk after `SHEX` if the container had none -- fxc's
/// chunk order is `RDEF, ISGN, OSGN, SHEX, SFI0, STAT`), and a refreshed checksum. Shared by the
/// `cb0`-remap and reprojection rewrites, which differ only in how they build the new chunks.
pub(super) fn reassemble(
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

/// Emits `o<pos> = M_eye · rClip` as four `dp4`s, one clip component each: `dp4 o<pos>.{x,y,z,w},
/// cb13[rBase.x + (MEYE_ROW_BASE + j)], rClip.xyzw`. `rBase.x` holds `4*eye`, so the immediate row
/// index selects the eye's `M_eye` block relative to it. Shared by the vertex and domain reprojection
/// rewrites: `pos_register` is the `SV_Position` output, `temp_base` is `rBase` (with `rClip` at
/// `temp_base + 1`).
pub(super) fn emit_meye_epilogue(out: &mut Vec<u32>, pos_register: u32, temp_base: u32) {
    let pos = pos_register;
    let rbase = temp_base;
    let rclip = temp_base + 1;
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
pub(super) fn rewrite_reproject_instruction(
    tokens: &[u32],
    insn: &Instruction<'_>,
    pos_register: u32,
    temp_base: u32,
    out: &mut Vec<u32>,
) -> Result<(), DxbcError> {
    let rclip = temp_base + 1;
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
            && tokens.get(reg_at) == Some(&pos_register);
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

/// Reads the register index of a declaration's operand (`dcl_input v2`, `dcl_constantbuffer
/// cb0[38]`, ...): the first index token after the operand token and any extended operand token.
pub(super) fn declared_register(tokens: &[u32], insn: &Instruction<'_>) -> Result<u32, DxbcError> {
    let at = insn.start + 1;
    let tok = *tokens.get(at).ok_or(DxbcError::UnexpectedEndOfTokens)?;
    let extended = ((tok >> 31) & 1) as usize;
    tokens
        .get(at + 1 + extended)
        .copied()
        .ok_or(DxbcError::UnexpectedEndOfTokens)
}

/// One `ISGN`/`OSGN` element to append.
pub(super) struct SignatureElement<'a> {
    pub(super) name: &'a str,
    pub(super) semantic_index: u32,
    pub(super) system_value: u32,
    pub(super) component_type: u32,
    pub(super) register: u32,
    pub(super) mask: u8,
    /// The read mask for inputs, or the never-written mask for outputs.
    pub(super) rw_mask: u8,
}

/// The fixed size of an `ISGN`/`OSGN` element record.
const SIGNATURE_ELEMENT_LEN: usize = 24;

/// Appends an element to an `ISGN`/`OSGN` chunk body: the element table grows by one record (which
/// shifts every name offset), the new semantic name lands at the end of the string blob, and the
/// chunk is re-padded to a dword boundary with `0xAB` as fxc does.
pub(super) fn append_signature_element(
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
pub(super) fn signature_register(
    body: &[u8],
    name: &str,
    semantic_index: u32,
) -> Option<(u32, u8)> {
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

/// The highest register index any element of an `ISGN`/`OSGN` chunk occupies, or `None` if the chunk
/// is empty. The authoritative count of used registers -- unlike a `dcl_output` scan, it also counts
/// signature-declared-but-unwritten slots, so the next free register is `max + 1`.
pub(super) fn max_signature_register(body: &[u8]) -> Option<u32> {
    if body.len() < 8 {
        return None;
    }
    let count = u32::from_le_bytes(body[0..4].try_into().ok()?) as usize;
    let table = u32::from_le_bytes(body[4..8].try_into().ok()?) as usize;
    (0..count)
        .filter_map(|i| {
            let rec = table.checked_add(i.checked_mul(SIGNATURE_ELEMENT_LEN)?)?;
            let record = body.get(rec..rec + SIGNATURE_ELEMENT_LEN)?;
            Some(u32::from_le_bytes(record[16..20].try_into().ok()?))
        })
        .max()
}

/// Sets the component mask and rw-mask of the `ISGN`/`OSGN` element with the given semantic name and
/// index, in place. The record size is unchanged, so no name offset shifts -- unlike
/// [`append_signature_element`]. Used to widen the `TEXCOORD3` lane's mask to `.xyz`.
pub(super) fn widen_signature_mask(
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

// SM4/5 opcodes.
pub(super) const OPCODE_AND: u32 = 0x01;
pub(super) const OPCODE_IMUL: u32 = 0x26;
pub(super) const OPCODE_MOV: u32 = 0x36;
pub(super) const OPCODE_DCL_CONSTANT_BUFFER: u32 = 0x59;
pub(super) const OPCODE_DCL_INPUT: u32 = 0x5F;
pub(super) const OPCODE_DCL_INPUT_SGV: u32 = 0x60;
pub(super) const OPCODE_DCL_INPUT_SIV: u32 = 0x61;
pub(super) const OPCODE_DCL_OUTPUT: u32 = 0x65;
pub(super) const OPCODE_DCL_OUTPUT_SGV: u32 = 0x66;
pub(super) const OPCODE_DCL_OUTPUT_SIV: u32 = 0x67;
pub(super) const OPCODE_DCL_TEMPS: u32 = 0x68;
// Reprojection opcodes.
pub(super) const OPCODE_DP4: u32 = 0x11;
pub(super) const OPCODE_RET: u32 = 0x3E;
pub(super) const OPCODE_RETC: u32 = 0x25;

/// SM4/5 operand type: output register (`oN`). Reprojection renames `SV_Position` writes (which are
/// output-register destinations) to a temp by clearing this type field to `0` (TEMP).
pub(super) const OPERAND_TYPE_OUTPUT: u32 = 2;
/// `D3D_NAME_POSITION`, the `dcl_output_siv` trailing system-value name for `SV_Position`.
pub(super) const SB_NAME_POSITION: u32 = 1;

/// The `dynamicIndexed` access pattern, bit 11 of a `dcl_constantbuffer` opcode token. Required
/// because `cb13` is indexed through a register, unlike the game's `immediateIndexed` `cb0`.
pub(super) const CB_ACCESS_DYNAMIC_INDEXED: u32 = 1 << 11;

// SM4/5 system-value names, as they appear in `dcl_*_sgv`/`dcl_*_siv` trailing tokens.
pub(super) const SB_NAME_VIEWPORT_ARRAY_INDEX: u32 = 5;
pub(super) const SB_NAME_INSTANCE_ID: u32 = 8;

// Signature (`ISGN`/`OSGN`) field values.
pub(super) const SIGNATURE_SYSVALUE_VIEWPORT_ARRAY_INDEX: u32 = 5;
pub(super) const SIGNATURE_SYSVALUE_INSTANCE_ID: u32 = 8;
pub(super) const SIGNATURE_COMPONENT_UINT32: u32 = 1;

// Operand tokens for the injected code, byte-identical to what fxc emits (see the module docs).
/// `cbN[a][b]`: constant buffer, 2D immediate indices, 4-component swizzle `.xyzw`.
pub(super) const OPERAND_CB_2D_IMM: u32 = 0x0020_8E46;
/// `vN.x` as a source: input register, 1D immediate index, select-1 component `.x`.
pub(super) const OPERAND_INPUT_SELECT_X: u32 = 0x0010_100A;
/// `vN.x` in a declaration: input register, 1D immediate index, write-mask-form `.x` (declarations
/// use the mask form, not a swizzle).
pub(super) const OPERAND_INPUT_MASK_X: u32 = 0x0010_1012;
/// `oM.x` as a destination: output register, 1D immediate index, write mask `.x`.
pub(super) const OPERAND_OUTPUT_MASK_X: u32 = 0x0010_2012;
/// `oM.y`/`.z`/`.w` as a destination: [`OPERAND_OUTPUT_MASK_X`] with the write-mask nibble moved to
/// the y/z/w component. The reprojection epilogue writes one clip component per `dp4`.
pub(super) const OPERAND_OUTPUT_MASK_Y: u32 = 0x0010_2022;
pub(super) const OPERAND_OUTPUT_MASK_Z: u32 = 0x0010_2042;
pub(super) const OPERAND_OUTPUT_MASK_W: u32 = 0x0010_2082;
/// `rN.xyzw` as a source: temp register, 1D immediate index, full `.xyzw` swizzle (the saved clip
/// position each `M_eye` row dots against).
pub(super) const OPERAND_TEMP_SWIZZLE_XYZW: u32 = 0x0010_0E46;
/// `rN.x` as a destination: temp register, 1D immediate index, write mask `.x`.
pub(super) const OPERAND_TEMP_MASK_X: u32 = 0x0010_0012;
/// `rN.x` as a source (and as a relative index): temp register, 1D immediate index, select-1 `.x`.
pub(super) const OPERAND_TEMP_SELECT_X: u32 = 0x0010_000A;
/// A scalar 32-bit immediate (`l(k)`).
pub(super) const OPERAND_IMM32_SCALAR: u32 = 0x0000_4001;
/// The null destination (`imul`'s unused high-half result).
pub(super) const OPERAND_NULL: u32 = 0x0000_D000;

/// `D3D_SHADER_REQUIRES_VIEWPORT_AND_RT_ARRAY_INDEX_FROM_ANY_SHADER_FEEDING_RASTERIZER`, bit 13 of
/// the `SFI0` feature flags. Without it the viewport output is invalid from a vertex shader.
const SFI0_VIEWPORT_FROM_ANY_STAGE: u64 = 1 << 13;
