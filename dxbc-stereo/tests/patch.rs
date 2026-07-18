//! Validates the single-pass stereo rewrite against the game's model vertex shader and against the
//! fxc reference blob.
//!
//! The game shader is git-ignored local extract, so those tests skip when it is absent; the fxc
//! reference (`tests/data/ref_vs50.dxbc`, compiled from `tests/data/ref_vs.hlsl` with
//! a `D3DCompile` harness) is our own output and rides with the crate, so the encoding
//! byte-match tests always run.

use std::path::PathBuf;

use dxbc_stereo::{
    Dxbc, DxbcError, OperandKind, ShaderStage, TokenStream, patch_vertex_shader, per_eye_refs,
    refresh_checksum,
};

/// The fxc reference: a vs_5_0 declaring `cb13` dynamicIndexed, reading `SV_InstanceID`, and
/// writing `SV_ViewportArrayIndex` -- the ground truth for the injected encodings.
const REFERENCE: &[u8] = include_bytes!("data/ref_vs50.dxbc");

/// The local extracted-shader directory, or `None` if it is not present.
fn shader_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tools/shaders/Shaders_F.shaders")
        .canonicalize()
        .ok()?;
    dir.is_dir().then_some(dir)
}

/// The canonical opaque model VS, patched -- or `None` when the extract is absent.
fn patched_model_vs() -> Option<Vec<u8>> {
    let dir = shader_dir()?;
    let blob = std::fs::read(dir.join("sh_0067_0002dcb0.dxbc")).expect("read sh_0067");
    Some(patch_vertex_shader(&blob).expect("patch sh_0067"))
}

/// The patched output must re-parse cleanly end to end: container, token stream, every instruction,
/// and every operand.
#[test]
fn patched_model_vs_reparses_cleanly() {
    let Some(patched) = patched_model_vs() else {
        eprintln!("skipping: extracted shaders not present");
        return;
    };
    let dxbc = Dxbc::parse(&patched).expect("parse patched container");
    let shex = dxbc.shader_chunk().expect("patched SHEX");
    let stream = TokenStream::new(shex.body(&patched)).expect("patched token stream");
    assert_eq!(stream.stage(), ShaderStage::Vertex);
    for insn in stream.instructions() {
        let insn = insn.expect("instruction decodes");
        for operand in insn.operands() {
            operand.expect("operand decodes");
        }
    }
}

/// After the rewrite no immediate per-eye `cb0` operand may remain, and all 17 references must have
/// become dynamic `cb13` operands.
#[test]
fn patched_model_vs_remaps_every_per_eye_operand() {
    let Some(patched) = patched_model_vs() else {
        eprintln!("skipping: extracted shaders not present");
        return;
    };
    assert_eq!(
        per_eye_refs(&patched).expect("per-eye scan"),
        vec![],
        "no immediate per-eye cb0 operands remain"
    );

    let dxbc = Dxbc::parse(&patched).expect("parse");
    let shex = dxbc.shader_chunk().expect("SHEX");
    let stream = TokenStream::new(shex.body(&patched)).expect("stream");
    let mut dynamic_cb13 = 0;
    for insn in stream.instructions() {
        for operand in insn.expect("instruction").operands() {
            if operand.expect("operand").kind
                == (OperandKind::ConstantBufferDynamic { register: 13 })
            {
                dynamic_cb13 += 1;
            }
        }
    }
    assert_eq!(dynamic_cb13, 17, "all 17 per-eye references now index cb13");
}

/// The patched container must declare the stereo interface: the `SFI0` viewport bit, an
/// `SV_InstanceID` input at the next free register (`v3`), and an `SV_ViewportArrayIndex` output at
/// the next free register (`o5`).
#[test]
fn patched_model_vs_declares_the_stereo_interface() {
    let Some(patched) = patched_model_vs() else {
        eprintln!("skipping: extracted shaders not present");
        return;
    };
    let dxbc = Dxbc::parse(&patched).expect("parse");

    let sfi0 = dxbc.chunk(b"SFI0").expect("SFI0 chunk present");
    let flags = u64::from_le_bytes(sfi0.body(&patched)[..8].try_into().expect("8 bytes"));
    assert_ne!(flags & (1 << 13), 0, "viewport-from-any-stage bit set");

    let isgn = signature_elements(dxbc.chunk(b"ISGN").expect("ISGN").body(&patched));
    let instance = isgn
        .iter()
        .find(|e| e.system_value == 8)
        .expect("ISGN has an INSTANCE_ID element");
    assert_eq!(instance.name, "SV_InstanceID");
    assert_eq!(instance.register, 3, "next free input register");

    let osgn = signature_elements(dxbc.chunk(b"OSGN").expect("OSGN").body(&patched));
    let viewport = osgn
        .iter()
        .find(|e| e.system_value == 5)
        .expect("OSGN has a VIEWPORT_ARRAY_INDEX element");
    assert_eq!(viewport.name, "SV_ViewportArrayIndex");
    assert_eq!(viewport.register, 5, "next free output register");

    // The pre-existing elements must survive the append, name offsets intact.
    assert_eq!(isgn.len(), 4);
    assert_eq!(isgn[0].name, "POSITION");
    assert_eq!(osgn.len(), 6);
    assert_eq!(osgn[0].name, "SV_Position");
}

/// The injected structures must byte-match the fxc reference: the signature element records (up to
/// the register and name offset, which legitimately differ), the semantic-name strings, the `SFI0`
/// body, and the declaration tokens (up to the register index token).
#[test]
fn injected_encodings_match_the_fxc_reference() {
    let Some(patched) = patched_model_vs() else {
        eprintln!("skipping: extracted shaders not present");
        return;
    };
    let ref_dxbc = Dxbc::parse(REFERENCE).expect("parse reference");
    let dxbc = Dxbc::parse(&patched).expect("parse patched");

    // SFI0: the reference container carries exactly the viewport bit, as must the patched output
    // (the game shader had no SFI0 to merge with).
    assert_eq!(
        dxbc.chunk(b"SFI0").expect("patched SFI0").body(&patched),
        ref_dxbc
            .chunk(b"SFI0")
            .expect("reference SFI0")
            .body(REFERENCE),
    );

    // Signature records: identical apart from the name offset (bytes 0..4) and register (16..20).
    let mask_position_fields = |mut record: [u8; 24]| {
        record[0..4].fill(0);
        record[16..20].fill(0);
        record
    };
    for (tag, sysvalue) in [(b"ISGN", 8u32), (b"OSGN", 5u32)] {
        let find = |blob: &[u8], dxbc: &Dxbc| {
            let body = dxbc
                .chunk(tag)
                .expect("signature chunk")
                .body(blob)
                .to_vec();
            let element = signature_elements(&body)
                .into_iter()
                .find(|e| e.system_value == sysvalue)
                .expect("stereo element present");
            (element.record, element.name)
        };
        let (ref_record, ref_name) = find(REFERENCE, &ref_dxbc);
        let (patched_record, patched_name) = find(&patched, &dxbc);
        assert_eq!(
            mask_position_fields(patched_record),
            mask_position_fields(ref_record),
            "signature record encoding matches the reference"
        );
        assert_eq!(patched_name, ref_name);
    }

    // Declaration tokens: `dcl_constantbuffer CB13[10] dynamicIndexed` matches wholesale;
    // `dcl_input_sgv`/`dcl_output_siv` match apart from the register index token.
    let ref_tokens = shex_tokens(REFERENCE);
    let patched_tokens = shex_tokens(&patched);
    let find_dcl = |tokens: &[Vec<u32>], opcode: u32, trailing: Option<u32>| -> Vec<u32> {
        tokens
            .iter()
            .find(|t| t[0] & 0x7FF == opcode && trailing.is_none_or(|v| *t.last().unwrap() == v))
            .expect("declaration present")
            .clone()
    };
    assert_eq!(
        find_dcl(&patched_tokens, 0x59, Some(10)),
        find_dcl(&ref_tokens, 0x59, Some(10)),
        "dcl_constantbuffer cb13[10] dynamicIndexed"
    );
    for (opcode, sysvalue) in [(0x60u32, 8u32), (0x67, 5)] {
        let reference = find_dcl(&ref_tokens, opcode, Some(sysvalue));
        let patched = find_dcl(&patched_tokens, opcode, Some(sysvalue));
        assert_eq!(patched[0], reference[0], "opcode token");
        assert_eq!(patched[1], reference[1], "operand token");
        assert_eq!(patched[3], reference[3], "system-value token");
    }
}

/// The rewrite must leave a valid checksum: refreshing the patched blob's checksum changes nothing.
#[test]
fn patched_model_vs_checksum_is_valid_and_refresh_is_idempotent() {
    let Some(patched) = patched_model_vs() else {
        eprintln!("skipping: extracted shaders not present");
        return;
    };
    let mut refreshed = patched.clone();
    refresh_checksum(&mut refreshed);
    assert_eq!(refreshed, patched, "the stored checksum is already correct");
}

/// Patching the same shader twice must fail cleanly rather than stack a second instance input.
#[test]
fn patching_twice_is_rejected() {
    let Some(patched) = patched_model_vs() else {
        eprintln!("skipping: extracted shaders not present");
        return;
    };
    assert!(matches!(
        patch_vertex_shader(&patched),
        Err(DxbcError::Cb13AlreadyDeclared | DxbcError::InstanceIdAlreadyDeclared)
    ));
}

/// The reference blob itself must not be patchable: it has no per-eye `cb0` rows (and already owns
/// `cb13`), so the rewrite must refuse it.
#[test]
fn reference_blob_is_rejected() {
    assert!(matches!(
        patch_vertex_shader(REFERENCE),
        Err(DxbcError::Cb13AlreadyDeclared | DxbcError::InstanceIdAlreadyDeclared)
    ));
}

/// A decoded `ISGN`/`OSGN` element, for assertions.
struct SignatureEntry {
    record: [u8; 24],
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
                record,
                name: String::from_utf8(body[name_offset..name_end].to_vec()).unwrap(),
                system_value: u32::from_le_bytes(record[8..12].try_into().unwrap()),
                register: u32::from_le_bytes(record[16..20].try_into().unwrap()),
            }
        })
        .collect()
}

/// Splits a container's shader chunk into per-instruction token vectors.
fn shex_tokens(blob: &[u8]) -> Vec<Vec<u32>> {
    let dxbc = Dxbc::parse(blob).expect("parse");
    let shex = dxbc.shader_chunk().expect("SHEX");
    let stream = TokenStream::new(shex.body(blob)).expect("stream");
    let body = shex.body(blob);
    let all: Vec<u32> = body
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    stream
        .instructions()
        .map(|insn| {
            let insn = insn.expect("instruction");
            all[insn.start..insn.end].to_vec()
        })
        .collect()
}
