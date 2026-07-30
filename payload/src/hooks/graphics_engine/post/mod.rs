//! Post-effects chain detours: the world post block, its per-effect hooks, and the auto-exposure
//! metering.

use re_utilities::hook_library::HookLibrary;

// `post_effects` is crate-visible so hooks::game can re-arm the once-per-dispatch world post-block
// gate at each dispatch begin.
pub(crate) mod post_effects;
// The auto-exposure / tone-mapping detours.
pub(crate) mod tone_mapping;

/// Bundle the post-chain detours into one hook library.
pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(tone_mapping::hook_library())
        .with_hook_library(post_effects::hook_library())
}
