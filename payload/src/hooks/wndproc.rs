use detours_macro::detour;
use re_utilities::hook_library::HookLibrary;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&WNDPROC_BINDER)
}

#[detour(address = 0x143_213_830)]
fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if let Some(egui_state) = crate::egui_impl::EguiState::get().as_mut() {
        egui_state.wndproc(hwnd, msg, wparam, lparam);
    }
    WNDPROC.get().unwrap().call(hwnd, msg, wparam, lparam)
}
