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
// The per-eye off-axis clip-to-view reconstruction fix (`PerspectiveFovInverse`); private, reached
// only through `hook_library` below.
mod reconstruction;
mod render_pass;
// `shader` is public so the debug UI can read its patched-shader count.
pub mod shader;
// `ssao` is crate-visible so hooks::game can read the recorded CSSAOPass pointer for the between-eye
// history-index restore.
pub(crate) mod ssao;
mod tone_mapping;

/// Bundle every CGraphicsEngine-area detour into one hook library, mirroring how the game groups
/// these classes.
pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(graphics_engine::hook_library())
        .with_hook_library(render_block::hook_library())
        .with_hook_library(reconstruction::hook_library())
        .with_hook_library(render_pass::hook_library())
        .with_hook_library(tone_mapping::hook_library())
        .with_hook_library(post_effects::hook_library())
        .with_hook_library(ssao::hook_library())
        .with_hook_library(shader::hook_library())
}
