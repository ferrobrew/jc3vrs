//! CGraphicsEngine-area detours, mirroring `jc3gi::graphics_engine`. Each leaf owns its detours in
//! its own [`HookLibrary`]; [`hook_library`] nests them into one parent library. The leaves are
//! grouped by pipeline stage -- [`scene`] (world geometry and culling), [`screen`] (screen-space
//! and fullscreen passes), and [`post`] (the post-effects chain) -- with the engine-core seams at
//! the root; each group's modules are re-exported here so consumers address them by the stable
//! `graphics_engine::<leaf>` path regardless of grouping (leaves nothing outside the group
//! references are not re-exported).

use re_utilities::hook_library::HookLibrary;

// Exposure and post-effect config structs, accessed by `tone_mapping` and `post_effects` via
// `Config::lock_query`.
pub(crate) mod config;
// `graphics_engine` stays public for hooks::game's BLOCK_FLIP reference. The inner name mirrors
// jc3gi (CGraphicsEngine in its own module), hence the module_inception allow.
#[allow(clippy::module_inception)]
pub mod graphics_engine;
mod render_pass;
// The three VR resolution levers (`ResizeTextures`, LR-particle, spot-light cone) for issue #8's
// pixelation.
mod resolution;
// `shader` is public so the debug UI can read its patched-shader count.
pub mod shader;
// Single-pass stereo render-thread detours (the cb13 mirror; experimental, gated off by default).
mod single_pass;

mod post;
mod scene;
mod screen;

pub(crate) use post::post_effects;
pub(crate) use scene::render_block;
pub use scene::terrain;
pub(crate) use screen::{clustered_lighting, reconstruction, ss_decal, ssao};

/// Bundle every CGraphicsEngine-area detour into one hook library, mirroring how the game groups
/// these classes.
pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(graphics_engine::hook_library())
        .with_hook_library(scene::hook_library())
        .with_hook_library(screen::hook_library())
        .with_hook_library(post::hook_library())
        .with_hook_library(render_pass::hook_library())
        .with_hook_library(resolution::hook_library())
        .with_hook_library(shader::hook_library())
        .with_hook_library(single_pass::hook_library())
}
