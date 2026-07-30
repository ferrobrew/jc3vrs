//! Screen-space and fullscreen pass detours: the per-eye depth-reconstruction basis and the passes
//! that consume it.

use re_utilities::hook_library::HookLibrary;

// The atmospheric-scattering pass's per-eye re-issue under the collapse.
pub(crate) mod atmospheric_scattering;
// The clustered-lighting froxel tile-bounds fix for off-axis VR projections (issue #35);
// crate-visible so the shared `Graphics::Clear` / `SetRenderSetup` / `SetVertexProgramConstants`
// detours (which the trace and single-pass modules own) can consult the per-eye froxel split.
pub(crate) mod clustered_lighting;
// The remaining fullscreen depth-reconstruction passes' per-eye re-issue under the collapse (SSAO,
// SSR, subsurface skin, and the unreachable depth-of-field basis).
pub(crate) mod fullscreen_reconstruction;
// The per-eye off-axis clip-to-view reconstruction fix (`PerspectiveFovInverse`); crate-visible so
// the shared `SetRenderSetup` detour can re-derive an at-entry per-eye scissor mask from the target
// the bind just made current.
pub(crate) mod reconstruction;
// `ssao` is crate-visible so hooks::game can read the recorded CSSAOPass pointer for the
// between-eye history-index restore.
pub(crate) mod ssao;
// The screen-space decal blocks' per-eye depth reconstruction under the collapse; reached by
// `shader` for the paired permutation rewrite.
pub(crate) mod ss_decal;

/// Bundle the screen-space pass detours into one hook library.
pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(reconstruction::hook_library())
        .with_hook_library(clustered_lighting::hook_library())
        .with_hook_library(ss_decal::hook_library())
        .with_hook_library(atmospheric_scattering::hook_library())
        .with_hook_library(fullscreen_reconstruction::hook_library())
        .with_hook_library(ssao::hook_library())
}
