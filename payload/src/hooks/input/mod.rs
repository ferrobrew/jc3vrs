//! Input-area detours, mirroring `jc3gi::input`.

use re_utilities::hook_library::HookLibrary;

pub mod locomotion;

pub(crate) fn hook_library() -> HookLibrary {
    locomotion::hook_library()
}
