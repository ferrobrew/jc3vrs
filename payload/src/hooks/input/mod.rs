//! Input-area detours, mirroring `jc3gi::input`.

use re_utilities::hook_library::HookLibrary;

pub mod locomotion;
pub mod look;

pub(crate) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(locomotion::hook_library())
        .with_hook_library(look::hook_library())
}
