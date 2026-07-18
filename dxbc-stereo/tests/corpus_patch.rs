//! Validates the single-pass vertex-shader rewrite against the game's *entire* real shader set.
//!
//! Where `patch.rs` checks the transform on one canonical shader and `census.rs` checks the operand
//! analysis over the corpus, this runs the full [`patch_vertex_shader`] over every shader in the
//! extracted bundle and structurally validates each success: the output re-parses, carries the
//! `SFI0` viewport-routing feature bit, has **no** residual per-eye `cb0` operand (every one was
//! remapped to `cb13`), and its checksum is self-consistent. It is the offline proof that the
//! rewriter survives contact with the real shader set before any of it reaches the game.
//!
//! The bundle is game-derived and git-ignored, so the test reads the local extract
//! (`tools/shaders/Shaders_F.shaders/`) and skips cleanly when absent (e.g. in CI).

use std::{collections::BTreeMap, path::PathBuf};

use dxbc_stereo::{Dxbc, DxbcError, ShaderStage, TokenStream, patch_vertex_shader, per_eye_refs};

/// `SFI0` bit 13 -- `VPAndRTArrayIndexFromAnyShaderFeedingRasterizer`, which every patched shader
/// must declare so its `SV_ViewportArrayIndex` output is valid.
const SFI0_VIEWPORT_BIT: u64 = 1 << 13;

/// The local extracted-shader directory, or `None` if it is not present.
fn shader_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tools/shaders/Shaders_F.shaders")
        .canonicalize()
        .ok()?;
    dir.is_dir().then_some(dir)
}

/// Whether a blob is a vertex shader (an `SHEX`/`SHDR` chunk whose program type is vertex).
fn is_vertex_shader(blob: &[u8]) -> bool {
    Dxbc::parse(blob)
        .ok()
        .and_then(|dxbc| dxbc.shader_chunk())
        .and_then(|shex| TokenStream::new(shex.body(blob)).ok())
        .is_some_and(|stream| stream.stage() == ShaderStage::Vertex)
}

/// Structurally validate one patched blob. Returns `Err(reason)` on the first invariant that fails.
fn validate_patch(patched: &[u8]) -> Result<(), String> {
    let dxbc = Dxbc::parse(patched).map_err(|e| format!("output does not re-parse: {e}"))?;

    let sfi0 = dxbc
        .chunk(b"SFI0")
        .ok_or_else(|| "output has no SFI0 chunk".to_string())?;
    let body = sfi0.body(patched);
    if body.len() < 8 {
        return Err(format!("SFI0 body is {} bytes, expected >= 8", body.len()));
    }
    let flags = u64::from_le_bytes(body[..8].try_into().expect("8 bytes"));
    if flags & SFI0_VIEWPORT_BIT == 0 {
        return Err(format!("SFI0 viewport bit not set (flags {flags:#x})"));
    }

    // The whole point of the rewrite: no per-eye cb0 operand may survive -- each must have become a
    // cb13 reference. A non-empty result means the transform missed an operand.
    let residual =
        per_eye_refs(patched).map_err(|e| format!("per_eye_refs on the output failed: {e}"))?;
    if !residual.is_empty() {
        let rows: Vec<u32> = residual.iter().map(|r| r.row).collect();
        return Err(format!(
            "{} residual per-eye cb0 operands at rows {rows:?}",
            residual.len()
        ));
    }

    // The token stream must still walk cleanly end to end.
    let shex = dxbc
        .shader_chunk()
        .ok_or_else(|| "output has no shader chunk".to_string())?;
    let stream = TokenStream::new(shex.body(patched))
        .map_err(|e| format!("output SHEX does not parse: {e}"))?;
    for insn in stream.instructions() {
        let insn = insn.map_err(|e| format!("output instruction walk failed: {e}"))?;
        for operand in insn.operands() {
            operand.map_err(|e| format!("output operand walk failed: {e}"))?;
        }
    }

    Ok(())
}

/// Run the full rewrite over every vertex shader in the bundle: every success must validate, and the
/// only tolerated failures are the two documented deferrals -- `NoPerEyeReferences` (the baked-WVP /
/// no-position families) and `InstanceIdAlreadyDeclared` (shaders that already consume
/// `SV_InstanceID` for their own instancing, whose `>> 1` consumer rewrite is a later phase). Both
/// are left double-drawn. Any *other* error, or any structurally-invalid output, fails the test with
/// the offending shader named.
#[test]
fn corpus_patch_is_sound() {
    let Some(dir) = shader_dir() else {
        eprintln!("skipping: extracted shaders not present");
        return;
    };

    let mut vs_total = 0usize;
    let mut patched = 0usize;
    let mut no_refs = 0usize;
    let mut instance_id = 0usize;
    let mut other_errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut invalid: Vec<String> = Vec::new();

    for entry in std::fs::read_dir(&dir).expect("read shader dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_none_or(|e| e != "dxbc") {
            continue;
        }
        let blob = std::fs::read(&path).expect("read blob");
        if !is_vertex_shader(&blob) {
            continue;
        }
        vs_total += 1;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();

        match patch_vertex_shader(&blob) {
            Ok(out) => {
                patched += 1;
                if let Err(reason) = validate_patch(&out) {
                    invalid.push(format!("{name}: {reason}"));
                }
            }
            Err(DxbcError::NoPerEyeReferences) => no_refs += 1,
            Err(DxbcError::InstanceIdAlreadyDeclared) => instance_id += 1,
            Err(e) => other_errors.entry(e.to_string()).or_default().push(name),
        }
    }

    eprintln!(
        "corpus patch: {vs_total} VS -> {patched} patched, {no_refs} no-per-eye-refs, \
         {instance_id} instance-id-deferred, {} other-errored, {} structurally-invalid",
        other_errors.values().map(Vec::len).sum::<usize>(),
        invalid.len(),
    );
    for (err, shaders) in &other_errors {
        eprintln!(
            "  error kind '{err}': {} shaders, e.g. {:?}",
            shaders.len(),
            &shaders[..shaders.len().min(3)]
        );
    }
    for line in &invalid {
        eprintln!("  invalid {line}");
    }

    assert_eq!(vs_total, 455, "total vertex shaders in the bundle");
    assert!(
        invalid.is_empty(),
        "{} patched shaders are structurally invalid",
        invalid.len()
    );
    assert!(
        other_errors.is_empty(),
        "{} shaders errored outside the two documented deferrals",
        other_errors.values().map(Vec::len).sum::<usize>(),
    );
    assert_eq!(
        patched + no_refs + instance_id,
        455,
        "every VS is patched, has no per-eye refs, or is an instance-id deferral"
    );
}
