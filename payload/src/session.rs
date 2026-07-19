//! The per-session output directory: one timestamped folder beside the payload DLL under which every
//! disk artifact the mod produces -- the log, crash dumps, profiler captures, render traces, grapple
//! telemetry, and screenshots -- is grouped, so a run's outputs share an unambiguous provenance
//! instead of scattering timestamped files across the DLL directory.
//!
//! The root is resolved once, at startup ([`init`], before logging installs), so every artifact of a
//! run lands under the same `sessions/<timestamp>/` folder. Single-file artifacts (the log, the crash
//! log) sit directly in the root via [`dir`]; artifact kinds that emit several files get a named
//! subdirectory via [`subdir`]. Both create their directory only when first written, so a run only
//! materializes the folders it actually uses.

use std::{path::PathBuf, sync::OnceLock};

/// Resolve this session's output root eagerly so the timestamp is stamped at startup rather than at
/// the first artifact write. Call once early in startup, before logging installs. Does not create the
/// directory -- that happens lazily when the first artifact is written.
pub fn init() {
    let _ = root();
}

/// A fresh local wall-clock timestamp string, `YYYY-MM-DD_HH-MM-SS` -- the one format every session
/// artifact stamps with. The session root uses it once at startup; callers that disambiguate several
/// captures within a run (profiler dumps, render traces, screenshot batches, grapple captures) name
/// their file or subfolder with a fresh one.
pub fn stamp() -> String {
    jiff::Zoned::now().strftime("%Y-%m-%d_%H-%M-%S").to_string()
}

/// This session's output root (`sessions/<timestamp>/`), created if missing. For artifacts that live
/// directly in the root -- the log and the crash log. `None` if the root is unavailable (the DLL path
/// could not be resolved) or the directory could not be created.
pub fn dir() -> Option<PathBuf> {
    let dir = root()?.clone();
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// A named subdirectory of the session root (`"profile"`, `"screenshots"`, `"traces"`, `"grapple"`),
/// created if missing. For artifact kinds that emit several files. `None` if the root is unavailable
/// or the directory could not be created.
pub fn subdir(name: &str) -> Option<PathBuf> {
    let dir = root()?.join(name);
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// This session's output root: `sessions/<local-timestamp>/` beside the payload DLL, or `None` if the
/// DLL path could not be resolved. Resolved once on first access and cached, so the timestamp is the
/// run's start time regardless of which artifact writes first. Computing the path does not create it.
fn root() -> Option<&'static PathBuf> {
    static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();
    ROOT.get_or_init(|| {
        let dir = crate::module::get_path()?
            .parent()?
            .join("sessions")
            .join(stamp());
        Some(dir)
    })
    .as_ref()
}
