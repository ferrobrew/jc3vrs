use crate::gfx::{GfxFile, Tag, read_string, read_u16_le, read_u30, read_u32_le};

/// Human-readable name for a SWF/GFX tag code
pub fn tag_name(code: u16) -> &'static str {
    match code {
        0 => "End",
        1 => "ShowFrame",
        2 => "DefineShape",
        4 => "PlaceObject",
        5 => "RemoveObject",
        6 => "DefineBits",
        9 => "SetBackgroundColor",
        12 => "DoAction",
        18 => "SoundStreamBlock",
        21 => "DefineBitsJPEG2",
        22 => "DefineShape2",
        24 => "Protect",
        26 => "PlaceObject2",
        28 => "RemoveObject2",
        32 => "DefineShape3",
        34 => "DefineButton2",
        35 => "DefineBitsJPEG3",
        37 => "DefineEditText",
        39 => "DefineSprite",
        43 => "FrameLabel",
        45 => "SoundStreamHead2",
        46 => "DefineMorphShape",
        48 => "DefineFont2",
        56 => "ExportAssets",
        57 => "ImportAssets",
        69 => "FileAttributes",
        70 => "PlaceObject3",
        73 => "DefineFontAlignZones",
        74 => "CSMTextSettings",
        75 => "DefineFont3",
        76 => "SymbolClass",
        77 => "StackTrace",
        78 => "DoABC",
        82 => "DoABC",
        83 => "DefineFont4",
        84 => "DefineMorphShape2",
        86 => "DefineSceneAndFrameLabelData",
        87 => "DefineBinaryData",
        88 => "DefineFontName",
        89 => "StartSound2",
        _ => "Unknown",
    }
}

/// Dump all interesting tag contents
pub fn dump(gfx: &GfxFile, output_dir: &Option<std::path::PathBuf>) -> anyhow::Result<()> {
    for (i, tag) in gfx.tags.iter().enumerate() {
        match tag.code {
            76 => dump_symbol_class(tag)?,
            56 => dump_export_assets(tag)?,
            78 | 82 => dump_do_abc(tag, i, output_dir)?,
            39 => dump_define_sprite(tag)?,
            86 => dump_scene_and_frame_labels(tag)?,
            26 | 70 => dump_place_object(tag, tag.code)?,
            69 => dump_file_attributes(tag)?,
            _ => {}
        }
    }
    Ok(())
}

fn dump_file_attributes(tag: &Tag) -> anyhow::Result<()> {
    if tag.data.is_empty() {
        return Ok(());
    }
    let flags = tag.data[0];
    println!("=== FileAttributes ===");
    println!("  use_network:   {}", flags & 0x01 != 0);
    println!("  actionscript3: {}", flags & 0x08 != 0);
    println!("  has_metadata:  {}", flags & 0x10 != 0);
    println!();
    Ok(())
}

fn dump_symbol_class(tag: &Tag) -> anyhow::Result<()> {
    let mut pos = 0;
    let count = read_u16_le(&tag.data, &mut pos) as usize;
    println!("=== SymbolClass ({count} mappings) ===");
    for _ in 0..count {
        if pos + 2 > tag.data.len() {
            break;
        }
        let tag_id = read_u16_le(&tag.data, &mut pos);
        let class_name = read_string(&tag.data, &mut pos);
        println!("  {tag_id:>5} -> {class_name}");
    }
    println!();
    Ok(())
}

fn dump_export_assets(tag: &Tag) -> anyhow::Result<()> {
    let mut pos = 0;
    let count = read_u16_le(&tag.data, &mut pos) as usize;
    println!("=== ExportAssets ({count} exports) ===");
    for _ in 0..count {
        if pos + 2 > tag.data.len() {
            break;
        }
        let tag_id = read_u16_le(&tag.data, &mut pos);
        let name = read_string(&tag.data, &mut pos);
        println!("  {tag_id:>5} -> {name}");
    }
    println!();
    Ok(())
}

fn dump_scene_and_frame_labels(tag: &Tag) -> anyhow::Result<()> {
    let mut pos = 0;
    let scene_count = read_u30(&tag.data, &mut pos) as usize;
    println!("=== DefineSceneAndFrameLabelData ({scene_count} scenes) ===");
    for i in 0..scene_count {
        let offset = read_u30(&tag.data, &mut pos);
        let name = read_string(&tag.data, &mut pos);
        println!("  scene {i}: offset={offset} name=\"{name}\"");
    }
    let frame_label_count = read_u30(&tag.data, &mut pos) as usize;
    for i in 0..frame_label_count {
        let frame_num = read_u30(&tag.data, &mut pos);
        let label = read_string(&tag.data, &mut pos);
        println!("  frame_label {i}: frame={frame_num} label=\"{label}\"");
    }
    println!();
    Ok(())
}

fn dump_place_object(tag: &Tag, code: u16) -> anyhow::Result<()> {
    // PlaceObject2 (26) and PlaceObject3 (70) share the same core structure
    let (flags, mut pos) = if code == 70 {
        // PlaceObject3 has 2 flag bytes
        (tag.data[0], 2)
    } else {
        (tag.data[0], 1)
    };

    let _has_clip_actions = flags & 0x80 != 0;
    let _has_clip_depth = flags & 0x40 != 0;
    let has_name = flags & 0x20 != 0;
    let has_ratio = flags & 0x10 != 0;
    let has_cxform = flags & 0x08 != 0;
    let has_matrix = flags & 0x04 != 0;
    let has_character = flags & 0x02 != 0;
    let move_flag = flags & 0x01 != 0;

    let depth = read_u16_le(&tag.data, &mut pos);

    let mut char_id_str = String::new();
    if has_character {
        let char_id = read_u16_le(&tag.data, &mut pos);
        char_id_str = format!("char={char_id} ");
    }

    // Skip matrix (if present): 6 floats as fixed-point — variable size, but
    // we only need the name. Skip by reading until we find it.
    // Matrix: nbits(5) + 6 fields of nbits each, plus optional scale/rotate
    if has_matrix {
        // Read the matrix: scale + rotate + translate
        // The matrix starts with a HasScale bit, then optional HasRotate
        // For simplicity, skip by reading all fields we know the format of
        skip_matrix(&tag.data, &mut pos);
    }

    if has_cxform {
        skip_cxform(&tag.data, &mut pos);
    }

    let mut name_str = String::new();
    if has_name {
        let name = read_string(&tag.data, &mut pos);
        name_str = format!("name=\"{name}\" ");
    }

    if has_ratio {
        let _ratio = read_u16_le(&tag.data, &mut pos);
    }

    if !name_str.is_empty() || has_character {
        println!(
            "  Place depth={depth} {char_id_str}{name_str}{}",
            if move_flag { "(move)" } else { "" }
        );
    }

    Ok(())
}

/// Skip a SWF MATRIX record, advancing pos past it.
fn skip_matrix(data: &[u8], pos: &mut usize) {
    if *pos >= data.len() {
        return;
    }
    // HasScale (1 bit)
    let has_scale = (data[*pos] >> 7) & 1;
    let mut bit_pos = 1;
    if has_scale == 1 {
        let nbits = read_ubits(data, pos, &mut bit_pos, 5) as usize;
        bit_pos += nbits * 2; // scaleX, scaleY
    }
    // HasRotate (1 bit)
    let has_rotate = read_ubits(data, pos, &mut bit_pos, 1);
    if has_rotate == 1 {
        let nbits = read_ubits(data, pos, &mut bit_pos, 5) as usize;
        bit_pos += nbits * 2; // rotateSkew0, rotateSkew1
    }
    // Translate (always present): nbits(5) + translateX + translateY
    let nbits = read_ubits(data, pos, &mut bit_pos, 5) as usize;
    bit_pos += nbits * 2; // translateX, translateY

    // Advance byte position past the consumed bits
    *pos += bit_pos.div_ceil(8);
}

/// Skip a SWF CXFORM record, advancing pos past it.
fn skip_cxform(data: &[u8], pos: &mut usize) {
    if *pos >= data.len() {
        return;
    }
    // HasAddTerms (1 bit) + HasMultTerms (1 bit) + Nbits (4 bits)
    let has_add = (data[*pos] >> 7) & 1;
    let has_mult = (data[*pos] >> 6) & 1;
    let nbits = ((data[*pos] >> 2) & 0x0F) as usize;
    let bit_pos = 6;
    let terms = (has_add as usize + has_mult as usize) * 3; // r, g, b per type
    let total_bits = bit_pos + nbits * terms;
    *pos += total_bits.div_ceil(8);
}

/// Read `count` bits from data starting at byte `*pos`, bit offset `bit_pos`.
/// Does NOT advance `pos`; the caller must update both.
fn read_ubits(data: &[u8], pos: &usize, bit_pos: &mut usize, count: usize) -> u32 {
    let mut result = 0u32;
    for i in 0..count {
        let byte_idx = *pos + (*bit_pos + i) / 8;
        if byte_idx >= data.len() {
            break;
        }
        let bit_idx = 7 - ((*bit_pos + i) % 8);
        let bit = (data[byte_idx] >> bit_idx) & 1;
        result = (result << 1) | bit as u32;
    }
    *bit_pos += count;
    result
}

fn dump_define_sprite(tag: &Tag) -> anyhow::Result<()> {
    let mut pos = 0;
    let sprite_id = read_u16_le(&tag.data, &mut pos);
    let frame_count = read_u16_le(&tag.data, &mut pos);

    // The rest of the data is a tag list (same format as the main body)
    // Parse sub-tags to find PlaceObject entries with depth info
    let sub_tags = parse_sub_tags(&tag.data[pos..])?;

    let mut placements: Vec<(u16, u16, Option<String>)> = Vec::new(); // (depth, char_id, name)
    for st in &sub_tags {
        if st.code == 26 || st.code == 70 {
            // Parse PlaceObject depth + name from the sub-tag
            let (flags, mut sp) = if st.code == 70 {
                (st.data[0], 2usize)
            } else {
                (st.data[0], 1usize)
            };
            let has_character = flags & 0x02 != 0;
            let has_name = flags & 0x20 != 0;
            let has_matrix = flags & 0x04 != 0;
            let has_cxform = flags & 0x08 != 0;
            let has_ratio = flags & 0x10 != 0;

            if sp + 2 > st.data.len() {
                continue;
            }
            let depth = u16::from_le_bytes(st.data[sp..sp + 2].try_into().unwrap());
            sp += 2;

            let mut char_id = 0u16;
            if has_character && sp + 2 <= st.data.len() {
                char_id = u16::from_le_bytes(st.data[sp..sp + 2].try_into().unwrap());
                sp += 2;
            }

            // Skip matrix and cxform to get to the name
            if has_matrix {
                skip_matrix(&st.data, &mut sp);
            }
            if has_cxform {
                skip_cxform(&st.data, &mut sp);
            }

            let mut name = None;
            if has_name && sp < st.data.len() {
                name = Some(read_string(&st.data, &mut sp));
            }

            let _ = has_ratio;
            placements.push((depth, char_id, name));
        }
    }

    if !placements.is_empty() {
        // Look up the sprite's class name from the SymbolClass
        println!("=== DefineSprite id={sprite_id} frames={frame_count} ===");
        for (depth, char_id, name) in &placements {
            let name_part = name
                .as_ref()
                .map(|n| format!(" name=\"{n}\""))
                .unwrap_or_default();
            println!("  depth={depth} char={char_id}{name_part}");
        }
        println!();
    }
    Ok(())
}

fn parse_sub_tags(data: &[u8]) -> anyhow::Result<Vec<crate::gfx::Tag>> {
    let mut tags = Vec::new();
    let mut pos = 0;

    while pos + 2 <= data.len() {
        let tag_code_and_length = u16::from_le_bytes(data[pos..pos + 2].try_into().unwrap());
        pos += 2;

        let code = tag_code_and_length >> 6;
        let mut length = (tag_code_and_length & 0x3F) as usize;

        if length == 0x3F {
            if pos + 4 > data.len() {
                break;
            }
            length = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
            pos += 4;
        }

        if pos + length > data.len() {
            length = data.len().saturating_sub(pos);
        }

        let tag_data = data[pos..pos + length].to_vec();
        pos += length;

        tags.push(crate::gfx::Tag {
            code,
            data: tag_data,
        });

        if code == 0 {
            break;
        }
    }

    Ok(tags)
}

// ============================================================
// DoABC — AS3 bytecode
// ============================================================

fn dump_do_abc(
    tag: &Tag,
    index: usize,
    output_dir: &Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    // DoABC (tag 82): flags (u32) + name (null-terminated string) + ABC data
    // DoABC2/DoABCBeforeFrame (tag 78): same format
    let mut pos = 0;
    let flags = read_u32_le(&tag.data, &mut pos);
    let name = read_string(&tag.data, &mut pos);
    let abc_data = &tag.data[pos..];

    println!(
        "=== DoABC[{index}] name=\"{name}\" flags={flags} abc_size={} ===",
        abc_data.len()
    );

    // Parse the ABC file to extract interesting constant pool entries
    let abc = parse_abc(abc_data)?;
    dump_abc_summary(&abc);

    // Dump the ABC block to a file if requested
    if let Some(out_dir) = output_dir {
        std::fs::create_dir_all(out_dir)?;
        let fname = format!("abc-{index:03}-{name}.abc");
        let path = out_dir.join(&fname);
        std::fs::write(&path, abc_data)?;
        println!("  ABC block dumped to: {}", path.display());
    }

    println!();
    Ok(())
}

#[allow(dead_code)]
struct AbcFile {
    strings: Vec<String>,
    namespace_strings: Vec<String>,
    multinames: Vec<String>,
    method_names: Vec<String>,
}

fn parse_abc(data: &[u8]) -> anyhow::Result<AbcFile> {
    let mut pos = 0;

    // Header: minor_version (u16) + major_version (u16)
    let minor = read_u16_le(data, &mut pos);
    let major = read_u16_le(data, &mut pos);
    // We don't validate; just skip
    let _ = (minor, major);

    // --- Constant pool: integers ---
    let int_count = read_u30(data, &mut pos) as usize;
    for _ in 1..int_count {
        read_u30(data, &mut pos); // skip variable-length s32
    }

    // --- Constant pool: unsigned integers ---
    let uint_count = read_u30(data, &mut pos) as usize;
    for _ in 1..uint_count {
        read_u30(data, &mut pos);
    }

    // --- Constant pool: doubles ---
    let double_count = read_u30(data, &mut pos) as usize;
    for _ in 1..double_count {
        if pos + 8 > data.len() {
            break;
        }
        pos += 8;
    }

    // --- Constant pool: strings ---
    let string_count = read_u30(data, &mut pos) as usize;
    let mut strings = Vec::with_capacity(string_count);
    strings.push(String::new()); // index 0 is null/empty
    for _ in 1..string_count {
        let str_len = read_u30(data, &mut pos) as usize;
        if pos + str_len > data.len() {
            break;
        }
        let s = String::from_utf8_lossy(&data[pos..pos + str_len]).to_string();
        strings.push(s);
        pos += str_len;
    }

    // --- Constant pool: namespaces ---
    let ns_count = read_u30(data, &mut pos) as usize;
    let mut namespace_strings = Vec::with_capacity(ns_count);
    namespace_strings.push(String::new()); // index 0
    for _ in 1..ns_count {
        if pos >= data.len() {
            break;
        }
        let _kind = data[pos];
        pos += 1;
        let name_idx = read_u30(data, &mut pos) as usize;
        let name = if name_idx < strings.len() {
            strings[name_idx].clone()
        } else {
            String::new()
        };
        namespace_strings.push(name);
    }

    // --- Constant pool: namespace sets ---
    let ns_set_count = read_u30(data, &mut pos) as usize;
    for _ in 1..ns_set_count {
        let count = read_u30(data, &mut pos) as usize;
        for _ in 0..count {
            read_u30(data, &mut pos);
        }
    }

    // --- Constant pool: multinames ---
    let multiname_count = read_u30(data, &mut pos) as usize;
    let mut multinames = Vec::with_capacity(multiname_count);
    multinames.push(String::new()); // index 0
    for _ in 1..multiname_count {
        if pos >= data.len() {
            break;
        }
        let kind = data[pos];
        pos += 1;
        let mn = match kind {
            0x07 | 0x0D => {
                // QName / QNameA: ns_idx (u30) + name_idx (u30)
                let _ns_idx = read_u30(data, &mut pos);
                let name_idx = read_u30(data, &mut pos) as usize;
                if name_idx < strings.len() {
                    strings[name_idx].clone()
                } else {
                    String::new()
                }
            }
            0x0F | 0x10 => {
                // RTQName / RTQNameA: name_idx (u30)
                let name_idx = read_u30(data, &mut pos) as usize;
                if name_idx < strings.len() {
                    strings[name_idx].clone()
                } else {
                    String::new()
                }
            }
            0x11 | 0x12 => {
                // RTQNameL / RTQNameLA: no data
                String::new()
            }
            0x09 | 0x0E => {
                // Multiname / MultinameA: name_idx (u30) + ns_set_idx (u30)
                let name_idx = read_u30(data, &mut pos) as usize;
                read_u30(data, &mut pos);
                if name_idx < strings.len() {
                    strings[name_idx].clone()
                } else {
                    String::new()
                }
            }
            0x1B => {
                // GenericName: name_idx (u30) + param_count (u30) + params
                let _name_idx = read_u30(data, &mut pos);
                let param_count = read_u30(data, &mut pos) as usize;
                for _ in 0..param_count {
                    read_u30(data, &mut pos);
                }
                String::new()
            }
            0x1C | 0x1D => {
                // MultinameL / MultinameLA: ns_set_idx (u30)
                read_u30(data, &mut pos);
                String::new()
            }
            _ => String::new(),
        };
        multinames.push(mn);
    }

    // --- Methods ---
    // Skip the rest of the ABC body (methods, metadata, instances, classes,
    // scripts, method bodies). Full parsing is complex due to non-standard
    // GenericName encoding in Scaleform's GFX ABC variant. The string pool
    // and multiname names we already have are the most useful data.
    let _method_count = read_u30(data, &mut pos);

    // Collect class/method names from multinames (non-empty ones)
    let mut method_names: Vec<String> = multinames
        .iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect();
    method_names.sort();
    method_names.dedup();

    Ok(AbcFile {
        strings,
        namespace_strings,
        multinames,
        method_names,
    })
}

fn dump_abc_summary(abc: &AbcFile) {
    // Print interesting strings (filtering out noise)
    let interesting: Vec<&String> = abc
        .strings
        .iter()
        .filter(|s| s.len() >= 4 && s.chars().filter(|c| c.is_alphabetic()).count() >= 3)
        .collect();

    println!(
        "  strings: {} total, {} interesting",
        abc.strings.len(),
        interesting.len()
    );
    if !interesting.is_empty() {
        println!("  --- interesting strings ---");
        for s in &interesting {
            println!("    {s}");
        }
    }

    if !abc.method_names.is_empty() {
        let unique: Vec<&String> = {
            let mut seen = std::collections::HashSet::new();
            abc.method_names
                .iter()
                .filter(|s| !s.is_empty() && seen.insert(s.as_str()))
                .collect()
        };
        println!("  --- class/method names ({}) ---", unique.len());
        for name in &unique {
            println!("    {name}");
        }
    }
}
