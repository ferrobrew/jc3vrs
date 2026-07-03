//! Input-area detours, mirroring `jc3gi::input`.

use re_utilities::hook_library::HookLibrary;

pub mod locomotion;
pub mod look;

pub(crate) fn hook_library() -> HookLibrary {
    locomotion::hook_library().with_static_binder(&look::INPUT_DEVICE_MANAGER_UPDATE_BINDER)
}
