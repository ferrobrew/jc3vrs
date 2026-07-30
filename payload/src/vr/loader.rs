//! Loading the OpenXR loader DLL (the dynamic route: configured path → payload-adjacent →
//! platform default search), and the marker that separates "there is no loader at all" from every
//! other bring-up failure.

use anyhow::Context as _;
use openxr as xr;

use crate::vr::VrConfig;

/// Load the OpenXR loader (dynamic route). Uses [`VrConfig::loader_path`] if set, else
/// `openxr_loader.dll` next to the payload DLL, falling back to the platform default search
/// (`xr::Entry::load`) when no explicit path is configured and the payload-adjacent DLL is
/// missing or fails to load — a system-wide loader then still works. An explicit
/// `loader_path` does not fall back: the user asked for that loader specifically.
///
/// Every failure carries a [`LoaderUnavailable`] marker in its context chain; see there for why.
pub(super) fn load_entry(cfg: &VrConfig) -> anyhow::Result<xr::Entry> {
    if let Some(path) = cfg.loader_path.clone().map(std::path::PathBuf::from) {
        tracing::info!(target: "vr", loader = %path.display(), "loading the configured OpenXR loader");
        return unsafe { xr::Entry::load_from(&path) }
            .with_context(|| format!("loader at {}", path.display()))
            .context(LoaderUnavailable);
    }

    if let Some(path) =
        crate::module::get_path().and_then(|p| p.parent().map(|d| d.join("openxr_loader.dll")))
    {
        tracing::info!(target: "vr", loader = %path.display(), "loading the payload-adjacent OpenXR loader");
        match unsafe { xr::Entry::load_from(&path) } {
            Ok(entry) => return Ok(entry),
            Err(e) => {
                tracing::info!(
                    target: "vr",
                    "payload-adjacent loader unavailable ({e}); trying the default search path",
                );
            }
        }
    }

    tracing::info!(target: "vr", "loading the OpenXR loader from the default search path");
    unsafe { xr::Entry::load() }
        .context("loader on the default search path")
        .context(LoaderUnavailable)
}

/// Marker attached to every [`load_entry`] failure, so the bring-up can tell "there is no OpenXR
/// loader to call at all" apart from every other reason bring-up can fail.
///
/// The distinction matters because the two want opposite responses. A runtime that is not running,
/// a headset that is unplugged, a system that is not yet ready — those are transient, and retrying
/// on a cadence is right. A loader that will not load is a build or deployment fault: the DLL is
/// staged beside the payload by `scripts/fetch_openxr_loader.sh`, and a fresh `target/` directory
/// does not have it. No retry can fix that, so `VrState::try_bring_up` makes it fatal.
#[derive(Debug)]
pub(super) struct LoaderUnavailable;

impl std::fmt::Display for LoaderUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "no OpenXR loader could be loaded (stage one beside the payload with \
             scripts/fetch_openxr_loader.sh, or point vr.loader_path at one)"
        )
    }
}

impl std::error::Error for LoaderUnavailable {}
