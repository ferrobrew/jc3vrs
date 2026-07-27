//! CGraphicsEngine-area detours, mirroring `jc3gi::graphics_engine`. Each leaf owns its detours in
//! its own [`HookLibrary`]; [`hook_library`] nests them into one parent library.

use re_utilities::hook_library::HookLibrary;

// `graphics_engine` stays public for hooks::game's BLOCK_FLIP reference; the others are private --
// only `hook_library` below reaches their `extend`. The inner name mirrors jc3gi (CGraphicsEngine in
// its own module), hence the module_inception allow.
#[allow(clippy::module_inception)]
pub mod graphics_engine;
// `post_effects` is crate-visible so hooks::game can re-arm the once-per-dispatch world post-block
// gate at each dispatch begin.
pub(crate) mod post_effects;
// `render_block` is crate-visible so hooks::character can publish the facial classification bones.
pub(crate) mod render_block;
// The VR two-eye cull-frustum widening (`GetBFBCFrustumParamsForCameraAndTime`); private, reached
// only through `hook_library` below.
mod culling;
// The per-eye off-axis clip-to-view reconstruction fix (`PerspectiveFovInverse`); crate-visible so the
// shared `SetRenderSetup` detour can re-derive an at-entry per-eye scissor mask from the target the
// bind just made current.
pub(crate) mod reconstruction;
// The atmospheric-scattering pass's per-eye re-issue under the collapse; private, reached only
// through `hook_library` below.
mod atmospheric_scattering;
// The remaining fullscreen depth-reconstruction passes' per-eye re-issue under the collapse (SSAO,
// SSR, subsurface skin, and the unreachable depth-of-field basis); private, reached only through
// `hook_library` below.
mod fullscreen_reconstruction;
mod render_pass;
// The three VR resolution levers (`ResizeTextures`, LR-particle, spot-light cone) for issue #8's
// pixelation; private, reached only through `hook_library` below.
mod resolution;
// `shader` is public so the debug UI can read its patched-shader count.
pub mod shader;
// Single-pass stereo render-thread detours (the cb13 mirror; experimental, gated off by default);
// private, reached only through `hook_library` below.
mod single_pass;
// `ssao` is crate-visible so hooks::game can read the recorded CSSAOPass pointer for the between-eye
// history-index restore.
pub(crate) mod ssao;
mod tone_mapping;
// The clustered-lighting froxel tile-bounds fix for off-axis VR projections (issue #35); crate-visible
// so the shared `Graphics::Clear` / `SetRenderSetup` / `SetVertexProgramConstants` detours (which the
// trace and single-pass modules own) can consult the per-eye froxel split.
pub(crate) mod clustered_lighting;
// Diagnostic override of the base VolumetricTerrain color-pass hull-clip type (black cliff-wall tiles);
// private, reached only through `hook_library` below.
pub mod terrain;
// The legacy water blocks' per-eye screen-UV bias under the collapse; private, reached only through
// `hook_library` below.
mod water;
// The screen-space decal blocks' per-eye depth reconstruction under the collapse; private, reached
// through `hook_library` below and by `shader` for the paired permutation rewrite.
mod ss_decal;

/// Bundle every CGraphicsEngine-area detour into one hook library, mirroring how the game groups
/// these classes.
pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(graphics_engine::hook_library())
        .with_hook_library(render_block::hook_library())
        .with_hook_library(culling::hook_library())
        .with_hook_library(reconstruction::hook_library())
        .with_hook_library(render_pass::hook_library())
        .with_hook_library(resolution::hook_library())
        .with_hook_library(tone_mapping::hook_library())
        .with_hook_library(post_effects::hook_library())
        .with_hook_library(ssao::hook_library())
        .with_hook_library(shader::hook_library())
        .with_hook_library(single_pass::hook_library())
        .with_hook_library(clustered_lighting::hook_library())
        .with_hook_library(ss_decal::hook_library())
        .with_hook_library(atmospheric_scattering::hook_library())
        .with_hook_library(fullscreen_reconstruction::hook_library())
        .with_hook_library(terrain::hook_library())
        .with_hook_library(water::hook_library())
}
