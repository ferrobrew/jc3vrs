//! Validates the terrain tessellation eye-lane rewrites. Phase 2 rides the single-pass eye index
//! through the `TEXCOORD3.z` lane VS -> HS -> DS; this covers the VS originator
//! ([`inject_eye_forward_vertex_shader`]). The game shaders are a git-ignored local extract, so these
//! tests skip when absent.

use std::path::PathBuf;

use dxbc_stereo::{
    Dxbc, ShaderStage, TokenStream, inject_eye_forward_vertex_shader, reproject_domain_shader,
};

/// The local extracted-shader directory, or `None` if it is not present.
fn shader_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tools/shaders/Shaders_F.shaders")
        .canonicalize()
        .ok()?;
    dir.is_dir().then_some(dir)
}

/// The terrain tessellation VS (`sh_0188`), eye-injected -- or `None` when the extract is absent.
fn eye_injected_terrain_vs() -> Option<Vec<u8>> {
    let dir = shader_dir()?;
    let blob = std::fs::read(dir.join("sh_0188_0006d550.dxbc")).expect("read sh_0188");
    Some(inject_eye_forward_vertex_shader(&blob).expect("eye-inject sh_0188"))
}

/// The eye-injected VS must re-parse cleanly, and the injected `SV_InstanceID` declaration and eye
/// write must land after every declaration (not interleaved among them), immediately before the first
/// executable instruction.
#[test]
fn eye_injected_vs_reparses_and_injects_after_declarations() {
    let Some(out) = eye_injected_terrain_vs() else {
        eprintln!("skipping: extracted shaders not present");
        return;
    };
    let dxbc = Dxbc::parse(&out).expect("parse");
    let shex = dxbc.shader_chunk().expect("SHEX");
    let stream = TokenStream::new(shex.body(&out)).expect("stream");
    assert_eq!(stream.stage(), ShaderStage::Vertex);

    // Walk the instructions: once the first executable instruction is seen, no declaration may follow
    // (the injected `dcl_input_sgv` must sit inside the declaration block, and the `and` after it).
    let mut seen_executable = false;
    let mut saw_instance_id_decl = false;
    for insn in stream.instructions() {
        let insn = insn.expect("instruction decodes");
        for operand in insn.operands() {
            operand.expect("operand decodes");
        }
        if insn.is_declaration() {
            assert!(
                !seen_executable,
                "declaration (opcode {:#x}) after an executable instruction -- injection misplaced",
                insn.opcode
            );
            // dcl_input_sgv is opcode 0x60; the injected SV_InstanceID rides one of them.
            if insn.opcode == 0x60 {
                saw_instance_id_decl = true;
            }
        } else {
            seen_executable = true;
        }
    }
    assert!(
        saw_instance_id_decl,
        "the SV_InstanceID declaration was injected"
    );
}

/// The eye-injected VS must declare `SV_InstanceID` in its input signature and widen the `TEXCOORD3`
/// output lane to `.xyz` (the eye rides `.z`).
#[test]
fn eye_injected_vs_declares_the_lane_interface() {
    let Some(out) = eye_injected_terrain_vs() else {
        eprintln!("skipping: extracted shaders not present");
        return;
    };
    let dxbc = Dxbc::parse(&out).expect("parse");

    let isgn = signature_elements(dxbc.chunk(b"ISGN").expect("ISGN").body(&out));
    assert!(
        isgn.iter()
            .any(|e| e.name == "SV_InstanceID" && e.system_value == 8),
        "ISGN gains SV_InstanceID"
    );

    let osgn = signature_elements(dxbc.chunk(b"OSGN").expect("OSGN").body(&out));
    let lane = osgn
        .iter()
        .find(|e| e.name == "TEXCOORD" && e.semantic_index == 3)
        .expect("OSGN has TEXCOORD3");
    assert_eq!(lane.mask & 0x7, 0x7, "TEXCOORD3 widened to .xyz");
}

/// The terrain tessellation DS (`sh_1513`), reprojected -- or `None` when the extract is absent.
fn reprojected_terrain_ds() -> Option<Vec<u8>> {
    let dir = shader_dir()?;
    let blob = std::fs::read(dir.join("sh_1513_00a8d600.dxbc")).expect("read sh_1513");
    Some(reproject_domain_shader(&blob).expect("reproject sh_1513"))
}

/// The reprojected DS must re-parse cleanly, and every instruction (including the injected prologue,
/// the widened lane input, and the `M_eye` epilogue) must decode with a length that lands exactly on
/// the chunk boundary -- the guard against the instruction-length miscount that made D3DDisassemble
/// loop.
#[test]
fn reprojected_ds_reparses_cleanly() {
    let Some(out) = reprojected_terrain_ds() else {
        eprintln!("skipping: extracted shaders not present");
        return;
    };
    let dxbc = Dxbc::parse(&out).expect("parse");
    let shex = dxbc.shader_chunk().expect("SHEX");
    let stream = TokenStream::new(shex.body(&out)).expect("stream");
    assert_eq!(stream.stage(), ShaderStage::Domain);

    let body = shex.body(&out);
    let declared_len = u32::from_le_bytes(body[4..8].try_into().unwrap()) as usize;
    let mut last_end = 2; // past version + length header
    for insn in stream.instructions() {
        let insn = insn.expect("instruction decodes");
        for operand in insn.operands() {
            operand.expect("operand decodes");
        }
        last_end = insn.end;
    }
    // The walk must consume the token stream exactly -- a miscounted instruction length (the bug that
    // made D3DDisassemble loop) would leave the final instruction ending short of or past the count.
    assert_eq!(
        last_end, declared_len,
        "instructions must end exactly at the declared token length"
    );
}

/// The reprojected DS must widen its `TEXCOORD3` input lane to `.xyz` (the eye rides `.z`) and gain an
/// `SV_ViewportArrayIndex` output at a register free of the existing signature (not merely free of the
/// body's `dcl_output`s -- the shader has a declared-but-unwritten `TEXCOORD2` output).
#[test]
fn reprojected_ds_widens_lane_and_adds_viewport() {
    let Some(out) = reprojected_terrain_ds() else {
        eprintln!("skipping: extracted shaders not present");
        return;
    };
    let dxbc = Dxbc::parse(&out).expect("parse");

    let isgn = signature_elements(dxbc.chunk(b"ISGN").expect("ISGN").body(&out));
    let lane = isgn
        .iter()
        .find(|e| e.name == "TEXCOORD" && e.semantic_index == 3)
        .expect("ISGN has TEXCOORD3");
    assert_eq!(lane.mask & 0x7, 0x7, "TEXCOORD3 input widened to .xyz");

    let osgn = signature_elements(dxbc.chunk(b"OSGN").expect("OSGN").body(&out));
    let viewport = osgn
        .iter()
        .find(|e| e.name == "SV_ViewportArrayIndex" && e.system_value == 5)
        .expect("OSGN gains SV_ViewportArrayIndex");
    // Every other output register must differ -- the viewport must not alias a declared slot.
    assert!(
        osgn.iter()
            .filter(|e| e.name != "SV_ViewportArrayIndex")
            .all(|e| e.register != viewport.register),
        "viewport register {} aliases an existing output",
        viewport.register
    );
}

/// A decoded `ISGN`/`OSGN` element, for assertions.
struct SignatureEntry {
    name: String,
    semantic_index: u32,
    system_value: u32,
    register: u32,
    mask: u8,
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
                semantic_index: u32::from_le_bytes(record[4..8].try_into().unwrap()),
                system_value: u32::from_le_bytes(record[8..12].try_into().unwrap()),
                register: u32::from_le_bytes(record[16..20].try_into().unwrap()),
                mask: record[20],
            }
        })
        .collect()
}
