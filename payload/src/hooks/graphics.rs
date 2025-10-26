use detours_macro::detour;
use jc3gi::graphics_engine::device::Device;
use re_utilities::hook_library::HookLibrary;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&GRAPHICS_FLIP_BINDER)
}

#[detour(address = 0x145_34B_870)]
fn graphics_flip(device: *mut Device) -> u64 {
    if let Some(egui_state) = crate::egui_impl::EguiState::get().as_mut() {
        egui_state.render();
    }
    GRAPHICS_FLIP.get().unwrap().call(device)
}
