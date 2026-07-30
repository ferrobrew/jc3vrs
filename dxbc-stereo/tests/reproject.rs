//! Validates the reprojection rewrite ([`reproject_vertex_shader`]): the fallback for the baked-WVP /
//! terrain / GPU-indirect families that have no per-eye `cb0` operands. It renames the shader's own
//! `SV_Position` writes to a temp and post-multiplies by a per-eye `M_eye` before `ret`.
//!
//! The canonical target is the skinned-character VS `sh_0024`, the biggest visual case (NPCs): it
//! reads a CPU-baked `WorldViewProj` from `cb1` and writes `o0 = skinnedPos·cb1[4..7]` with no `cb0`
//! reference. The game shaders are a git-ignored local extract, so these tests skip when absent.

use dxbc_stereo::{
    Dxbc, DxbcError, MEYE_ROW_BASE, STEREO_REPROJ_CB_ROWS, ShaderStage, TokenStream,
    patch_vertex_shader, refresh_checksum, reproject_vertex_shader,
};

mod common;
use common::shader_dir;

/// The canonical skinned-character (baked-WVP) VS, reprojected -- or `None` when the extract is absent.
fn reprojected_character_vs() -> Option<Vec<u8>> {
    let dir = shader_dir()?;
    let blob = std::fs::read(dir.join("sh_0024_000126c0.dxbc")).expect("read sh_0024");
    // Precondition for this test's premise: the shader really is a no-`cb0` (reprojection) case.
    assert!(
        matches!(
            patch_vertex_shader(&blob),
            Err(DxbcError::NoPerEyeReferences)
        ),
        "sh_0024 should be a baked-WVP no-per-eye-refs shader"
    );
    Some(reproject_vertex_shader(&blob).expect("reproject sh_0024"))
}

/// The reprojected output must re-parse cleanly end to end.
#[test]
fn reprojected_vs_reparses_cleanly() {
    let Some(reprojected) = reprojected_character_vs() else {
        eprintln!("skipping: extracted shaders not present");
        return;
    };
    let dxbc = Dxbc::parse(&reprojected).expect("parse reprojected container");
    let shex = dxbc.shader_chunk().expect("reprojected SHEX");
    let stream = TokenStream::new(shex.body(&reprojected)).expect("reprojected token stream");
    assert_eq!(stream.stage(), ShaderStage::Vertex);
    for insn in stream.instructions() {
        let insn = insn.expect("instruction decodes");
        for operand in insn.operands() {
            operand.expect("operand decodes");
        }
    }
}

/// The reprojected container must declare the stereo interface: the `SFI0` viewport bit, an
/// `SV_InstanceID` input, an `SV_ViewportArrayIndex` output, and `cb13` at the full reprojection size.
#[test]
fn reprojected_vs_declares_the_stereo_interface() {
    let Some(reprojected) = reprojected_character_vs() else {
        eprintln!("skipping: extracted shaders not present");
        return;
    };
    let dxbc = Dxbc::parse(&reprojected).expect("parse");

    let sfi0 = dxbc.chunk(b"SFI0").expect("SFI0 chunk present");
    let flags = u64::from_le_bytes(sfi0.body(&reprojected)[..8].try_into().expect("8 bytes"));
    assert_ne!(flags & (1 << 13), 0, "viewport-from-any-stage bit set");

    let isgn = signature_elements(dxbc.chunk(b"ISGN").expect("ISGN").body(&reprojected));
    assert!(
        isgn.iter()
            .any(|e| e.system_value == 8 && e.name == "SV_InstanceID"),
        "ISGN gains SV_InstanceID"
    );
    let osgn = signature_elements(dxbc.chunk(b"OSGN").expect("OSGN").body(&reprojected));
    assert!(
        osgn.iter()
            .any(|e| e.system_value == 5 && e.name == "SV_ViewportArrayIndex"),
        "OSGN gains SV_ViewportArrayIndex"
    );

    // cb13 is declared at the full reprojection size (remap rows + the M_eye block).
    let cb13_size = cb13_declared_size(&reprojected).expect("cb13 declared");
    assert_eq!(cb13_size, STEREO_REPROJ_CB_ROWS);
}

/// After reprojection the `SV_Position` output must be written by exactly four `dp4`s (the `M_eye`
/// rows) and nothing else -- the shader's own position writes are renamed away to the `rClip` temp.
#[test]
fn reprojected_vs_position_is_only_the_meye_epilogue() {
    let Some(reprojected) = reprojected_character_vs() else {
        eprintln!("skipping: extracted shaders not present");
        return;
    };
    let pos_reg = position_output_register(&reprojected).expect("SV_Position output register");

    let dxbc = Dxbc::parse(&reprojected).expect("parse");
    let shex = dxbc.shader_chunk().expect("SHEX");
    let body = shex.body(&reprojected);
    let stream = TokenStream::new(body).expect("stream");
    let tokens: Vec<u32> = body
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();

    const OPCODE_DP4: u32 = 0x11;
    let mut writers: Vec<u32> = Vec::new();
    for insn in stream.instructions() {
        let insn = insn.expect("instruction");
        if insn.is_declaration() {
            continue; // the SV_Position output declaration is not a write.
        }
        for operand in insn.operands() {
            let operand = operand.expect("operand");
            if writes_output_register(&tokens, operand.token_offset, pos_reg) {
                writers.push(insn.opcode);
            }
        }
    }
    assert_eq!(
        writers,
        vec![OPCODE_DP4; 4],
        "SV_Position is written only by the four M_eye dp4s"
    );
}

/// The reprojected blob must carry a valid checksum, and reprojecting is one-shot: a reprojected
/// shader already declares `cb13`, so a second pass is refused.
#[test]
fn reprojected_vs_checksum_is_valid_and_second_pass_rejected() {
    let Some(reprojected) = reprojected_character_vs() else {
        eprintln!("skipping: extracted shaders not present");
        return;
    };
    let mut refreshed = reprojected.clone();
    refresh_checksum(&mut refreshed);
    assert_eq!(
        refreshed, reprojected,
        "the stored checksum is already correct"
    );

    assert!(matches!(
        reproject_vertex_shader(&reprojected),
        Err(DxbcError::Cb13AlreadyDeclared | DxbcError::InstanceIdAlreadyDeclared)
    ));
}

/// Sweeps the corpus: every no-`cb0` vertex shader that reprojects must produce a structurally sound
/// blob (re-parses, declares the interface, has the four-`dp4` epilogue). Reports the tally.
#[test]
fn corpus_reproject_is_sound() {
    let Some(dir) = shader_dir() else {
        eprintln!("skipping: extracted shaders not present");
        return;
    };
    let (mut reprojected, mut no_position, mut other_err, mut vs_total) = (0u32, 0u32, 0u32, 0u32);
    for entry in std::fs::read_dir(&dir).expect("read shader dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("dxbc") {
            continue;
        }
        let blob = std::fs::read(&path).expect("read shader");
        // Only the no-per-eye-refs vertex shaders are reprojection candidates.
        if !matches!(
            patch_vertex_shader(&blob),
            Err(DxbcError::NoPerEyeReferences)
        ) {
            continue;
        }
        vs_total += 1;
        match reproject_vertex_shader(&blob) {
            Ok(out) => {
                validate_reprojected(&out).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                reprojected += 1;
            }
            Err(DxbcError::NoPositionOutput) => no_position += 1,
            Err(_) => other_err += 1,
        }
    }
    eprintln!(
        "corpus reproject: {vs_total} no-refs VS -> {reprojected} reprojected, \
         {no_position} no-position, {other_err} other"
    );
    assert!(
        vs_total > 0,
        "corpus has no-refs vertex shaders to reproject"
    );
    assert_eq!(other_err, 0, "no unexpected reprojection failures");
}

/// Structural validation of a reprojected blob: re-parses, `SFI0` viewport bit set, `cb13` at the
/// reprojection size, and the `SV_Position` output written only by the four `M_eye` `dp4`s.
fn validate_reprojected(blob: &[u8]) -> Result<(), String> {
    let dxbc = Dxbc::parse(blob).map_err(|e| format!("parse: {e}"))?;
    let shex = dxbc.shader_chunk().ok_or("no SHEX")?;
    let body = shex.body(blob);
    let stream = TokenStream::new(body).map_err(|e| format!("stream: {e}"))?;
    for insn in stream.instructions() {
        let insn = insn.map_err(|e| format!("insn: {e}"))?;
        for operand in insn.operands() {
            operand.map_err(|e| format!("operand: {e}"))?;
        }
    }
    let sfi0 = dxbc.chunk(b"SFI0").ok_or("no SFI0")?;
    let flags = u64::from_le_bytes(sfi0.body(blob)[..8].try_into().map_err(|_| "short SFI0")?);
    if flags & (1 << 13) == 0 {
        return Err("SFI0 viewport bit not set".into());
    }
    if cb13_declared_size(blob) != Some(STEREO_REPROJ_CB_ROWS) {
        return Err("cb13 not declared at the reprojection size".into());
    }

    let pos_reg = position_output_register(blob).ok_or("no SV_Position output")?;
    let tokens: Vec<u32> = body
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    let mut writers = Vec::new();
    for insn in stream.instructions() {
        let insn = insn.map_err(|e| format!("insn: {e}"))?;
        if insn.is_declaration() {
            continue; // the SV_Position output declaration is not a write.
        }
        for operand in insn.operands() {
            let operand = operand.map_err(|e| format!("operand: {e}"))?;
            if writes_output_register(&tokens, operand.token_offset, pos_reg) {
                writers.push(insn.opcode);
            }
        }
    }
    if writers != vec![0x11u32; 4] {
        return Err(format!("SV_Position writers = {writers:?}, want four dp4"));
    }
    Ok(())
}

/// Whether the operand at `token_offset` is a write to output register `reg` (operand type OUTPUT,
/// 1D immediate register index).
fn writes_output_register(tokens: &[u32], token_offset: usize, reg: u32) -> bool {
    let tok = tokens[token_offset];
    let operand_type = (tok >> 12) & 0xFF;
    let index_dim = (tok >> 20) & 0x3;
    let rep = (tok >> 22) & 0x7;
    let extended = ((tok >> 31) & 1) as usize;
    operand_type == 2
        && index_dim == 1
        && rep == 0
        && tokens.get(token_offset + 1 + extended) == Some(&reg)
}

/// The `SV_Position` output register, read from the `OSGN` (system value 1 = `POSITION`).
fn position_output_register(blob: &[u8]) -> Option<u32> {
    signature_elements(Dxbc::parse(blob).ok()?.chunk(b"OSGN")?.body(blob))
        .into_iter()
        .find(|e| e.system_value == 1)
        .map(|e| e.register)
}

/// The `cb13` size declared by `dcl_constantbuffer CB13[n]`, if present.
fn cb13_declared_size(blob: &[u8]) -> Option<u32> {
    let dxbc = Dxbc::parse(blob).ok()?;
    let shex = dxbc.shader_chunk()?;
    let body = shex.body(blob);
    let tokens: Vec<u32> = body
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    let stream = TokenStream::new(body).ok()?;
    for insn in stream.instructions() {
        let insn = insn.ok()?;
        // dcl_constantbuffer: operand token, register (13), size.
        if insn.opcode == 0x59 && tokens.get(insn.start + 2) == Some(&13) {
            return tokens.get(insn.start + 3).copied();
        }
    }
    let _ = MEYE_ROW_BASE; // referenced so the layout constant stays test-visible.
    None
}

/// A decoded `ISGN`/`OSGN` element, for assertions.
struct SignatureEntry {
    name: String,
    system_value: u32,
    register: u32,
}

/// Decodes the elements of an `ISGN`/`OSGN` chunk body.
fn signature_elements(body: &[u8]) -> Vec<SignatureEntry> {
    let count = u32::from_le_bytes(body[0..4].try_into().unwrap()) as usize;
    let table = u32::from_le_bytes(body[4..8].try_into().unwrap()) as usize;
    (0..count)
        .map(|i| {
            let record: [u8; 24] = body[table + i * 24..][..24].try_into().unwrap();
            let name_offset = u32::from_le_bytes(record[0..4].try_into().unwrap()) as usize;
            let name_end = body[name_offset..]
                .iter()
                .position(|&b| b == 0)
                .map(|p| name_offset + p)
                .unwrap();
            SignatureEntry {
                name: String::from_utf8(body[name_offset..name_end].to_vec()).unwrap(),
                system_value: u32::from_le_bytes(record[8..12].try_into().unwrap()),
                register: u32::from_le_bytes(record[16..20].try_into().unwrap()),
            }
        })
        .collect()
}
