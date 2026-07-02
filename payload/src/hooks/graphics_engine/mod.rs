//! CGraphicsEngine-area detours, mirroring `jc3gi::graphics_engine`. Each leaf owns its detours and
//! contributes them via `extend`; [`hook_library`] bundles the four into one library.

use re_utilities::hook_library::HookLibrary;

// `graphics_engine` stays public for hooks::game's BLOCK_FLIP reference; the others are private --
// only `hook_library` below reaches their `extend`. The inner name mirrors jc3gi (CGraphicsEngine in
// its own module), hence the module_inception allow.
#[allow(clippy::module_inception)]
pub mod graphics_engine;
// `post_effects` is crate-visible so hooks::game can re-arm the once-per-dispatch world post-block
// gate at each dispatch begin.
pub(crate) mod post_effects;
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
    [
        graphics_engine::extend,
        render_pass::extend,
        tone_mapping::extend,
        post_effects::extend,
        ssao::extend,
        shader::extend,
    ]
    .into_iter()
    .fold(HookLibrary::new(), |library, extend| extend(library))
}
