//! Shared helpers for the tests that run against the game's own extracted shader bundle.
//!
//! The bundle is game-derived and git-ignored, so the tests read a local extract
//! (`tools/shaders/Shaders_F.shaders/`, produced by `tools/shaders/extract_dxbc.py`).

#![allow(dead_code)]

use std::path::PathBuf;

/// Set this to `1` to turn a missing shader extract into a skip instead of a failure.
///
/// The default is to fail. A silent skip made the whole corpus suite vacuously green on any machine
/// without the extract -- including a fresh clone and CI -- so an assertion as load-bearing as "all
/// 455 vertex shaders round-trip" never ran, and a real regression in the rewriter went unnoticed
/// until it was found by inspection.
pub const ALLOW_MISSING: &str = "JC3VR_ALLOW_MISSING_SHADER_CORPUS";

/// The local extracted-shader directory. Panics with instructions when it is absent, unless
/// [`ALLOW_MISSING`] is set, in which case it returns `None` and the caller skips.
pub fn shader_dir() -> Option<PathBuf> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tools/shaders/Shaders_F.shaders");
    match path.canonicalize() {
        Ok(dir) if dir.is_dir() => Some(dir),
        _ if std::env::var_os(ALLOW_MISSING).is_some() => {
            eprintln!("skipping: extracted shaders not present ({ALLOW_MISSING} is set)");
            None
        }
        _ => panic!(
            "the extracted shader bundle is missing at {}.\n\
             Extract it with:\n  \
             python3 tools/shaders/extract_dxbc.py \"<game dir>/Shaders_F.shader_bundle\"\n\
             Or set {ALLOW_MISSING}=1 to skip the corpus tests instead.",
            path.display(),
        ),
    }
}
