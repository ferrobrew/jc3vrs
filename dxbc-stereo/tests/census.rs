//! Validates the operand census against the game's real vertex shaders.
//!
//! The extracted shader bundle is game-derived content and git-ignored, so these tests read it from
//! the local extract (`tools/shaders/Shaders_F.shaders/`) and skip cleanly when it is absent (e.g.
//! in CI), so the crate still builds and its parser is exercised wherever the shaders exist.

use std::path::PathBuf;

use dxbc_stereo::{Dxbc, OperandKind, PerEyeRef, TokenStream, per_eye_refs};

/// The local extracted-shader directory, or `None` if it is not present.
fn shader_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tools/shaders/Shaders_F.shaders")
        .canonicalize()
        .ok()?;
    dir.is_dir().then_some(dir)
}

/// The canonical opaque model VS (`sh_0067`) must show exactly the per-eye operand fingerprint: the
/// camera position `cb0[4]` once, and each view-projection row `cb0[29..32]` four times.
#[test]
fn model_vs_has_the_expected_per_eye_fingerprint() {
    let Some(dir) = shader_dir() else {
        eprintln!("skipping: extracted shaders not present");
        return;
    };
    let blob = std::fs::read(dir.join("sh_0067_0002dcb0.dxbc")).expect("read sh_0067");
    let refs = per_eye_refs(&blob).expect("parse sh_0067");

    let count_of = |row: u32| refs.iter().filter(|r| r.row == row).count();
    assert_eq!(count_of(4), 1, "cb0[4] camera position");
    for row in [29, 30, 31, 32] {
        assert_eq!(count_of(row), 4, "cb0[{row}] view-projection row");
    }
    assert_eq!(refs.len(), 17, "total per-eye operand references");

    // The token offsets must be distinct and in program order.
    let mut offsets: Vec<usize> = refs.iter().map(|r: &PerEyeRef| r.token_offset).collect();
    let sorted = {
        let mut s = offsets.clone();
        s.sort_unstable();
        s
    };
    assert_eq!(offsets, sorted, "refs are returned in program order");
    offsets.dedup();
    assert_eq!(offsets.len(), 17, "operand token offsets are distinct");
}

/// Sweeping the whole corpus must parse every vertex shader without error and reproduce the
/// 155-shader model family that reads the global view-projection from `cb0[29..32]`.
#[test]
fn corpus_census_reproduces_the_model_family() {
    let Some(dir) = shader_dir() else {
        eprintln!("skipping: extracted shaders not present");
        return;
    };

    let mut vs_total = 0;
    let mut with_global_vp = 0;
    let mut parse_failures = 0;
    for entry in std::fs::read_dir(&dir).expect("read shader dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_none_or(|e| e != "dxbc") {
            continue;
        }
        let blob = std::fs::read(&path).expect("read blob");
        let Ok(dxbc) = Dxbc::parse(&blob) else {
            continue;
        };
        let Some(shex) = dxbc.shader_chunk() else {
            continue;
        };
        let Ok(stream) = TokenStream::new(shex.body(&blob)) else {
            parse_failures += 1;
            continue;
        };
        if stream.stage() != dxbc_stereo::ShaderStage::Vertex {
            continue;
        }
        vs_total += 1;

        let mut rows = std::collections::BTreeSet::new();
        let mut errored = false;
        for insn in stream.instructions() {
            let Ok(insn) = insn else {
                errored = true;
                break;
            };
            for operand in insn.operands() {
                match operand {
                    Ok(op) => {
                        if let OperandKind::ConstantBuffer {
                            register: 0,
                            element,
                        } = op.kind
                        {
                            rows.insert(element);
                        }
                    }
                    Err(_) => {
                        errored = true;
                        break;
                    }
                }
            }
        }
        if errored {
            parse_failures += 1;
            continue;
        }
        if [29, 30, 31, 32].iter().all(|r| rows.contains(r)) {
            with_global_vp += 1;
        }
    }

    eprintln!(
        "VS total {vs_total}, global-VP family {with_global_vp}, parse failures {parse_failures}"
    );
    assert_eq!(vs_total, 455, "total vertex shaders in the bundle");
    assert_eq!(parse_failures, 0, "every VS parses cleanly");
    assert_eq!(with_global_vp, 155, "the model family reading cb0[29..32]");
}
