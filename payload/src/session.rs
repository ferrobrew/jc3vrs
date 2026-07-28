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

/// A fresh local wall-clock timestamp string, `YYYY-MM-DD_HH-MM-SS_mmm` -- the one format every
/// session artifact stamps with. The session root uses it once at startup; callers that disambiguate
/// several captures within a run (profiler dumps, render traces, screenshot batches, grapple captures)
/// name their file or subfolder with a fresh one. The millisecond suffix keeps two stamps taken within
/// the same second -- notably an uninject/reinject cycle during development, which routinely lands
/// inside one wall-clock second -- from resolving to the same path.
pub fn stamp() -> String {
    jiff::Zoned::now()
        .strftime("%Y-%m-%d_%H-%M-%S_%3f")
        .to_string()
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
    debug_assert!(
        !name.contains(std::path::MAIN_SEPARATOR)
            && !name.contains('/')
            && !name.contains('\\')
            && name != "..",
        "session::subdir: name must be a simple directory name, got {name:?}"
    );
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

#[cfg(test)]
mod tests {
    use super::stamp;

    /// The format string is parsed at run time, so a directive the version of `jiff` in use does not
    /// accept would surface as a malformed session path on the startup path rather than as a build
    /// failure. Pin the shape instead: `YYYY-MM-DD_HH-MM-SS_mmm`, all digits, path-safe.
    #[test]
    fn stamp_is_second_and_millisecond_precise() {
        let stamp = stamp();
        let (date, rest) = stamp
            .split_once('_')
            .expect("date and time are `_`-separated");
        let (time, millis) = rest
            .split_once('_')
            .expect("a millisecond suffix is present");

        assert_eq!(date.len(), 10, "date is `YYYY-MM-DD`: {stamp}");
        assert_eq!(time.len(), 8, "time is `HH-MM-SS`: {stamp}");
        assert_eq!(millis.len(), 3, "milliseconds are three digits: {stamp}");
        assert!(millis.bytes().all(|b| b.is_ascii_digit()), "{stamp}");
        assert_eq!(date.matches('-').count(), 2, "{stamp}");
        assert_eq!(time.matches('-').count(), 2, "{stamp}");
    }

    /// `subdir` must reject path separators and parent-directory components so a caller cannot
    /// escape the session root. Under `debug_assertions` the invalid names panic; in release builds
    /// the test is skipped (the `debug_assert!` is a no-op there).
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "session::subdir: name must be a simple directory name")]
    fn subdir_rejects_path_separators() {
        let _ = super::subdir("../escape");
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "session::subdir: name must be a simple directory name")]
    fn subdir_rejects_nested_path() {
        let _ = super::subdir("a/b");
    }
}
