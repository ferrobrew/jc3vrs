use detours_macro::detour;
use re_utilities::hook_library::HookLibrary;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::{
        Input::KeyboardAndMouse::VK_F10,
        WindowsAndMessaging::{WM_KEYDOWN, WM_SYSKEYDOWN},
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&WNDPROC_BINDER)
}

#[detour(address = jc3gi::window::WndProc_ADDRESS)]
fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // F10 toggles the fullscreen stereo capture mode. Intercept it before egui or the game sees it:
    // F10 is a system key (it activates the menu bar via WM_SYSKEYDOWN), so consuming it here also
    // suppresses that default behaviour. Edge-detect on the previous-state bit (lparam bit 30) so
    // holding F10 toggles once, not on every repeat.
    if (msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN) && wparam.0 == VK_F10.0 as usize {
        let previous_down = (lparam.0 & 0x4000_0000) != 0;
        if !previous_down {
            // Release egui's input capture if held, so the game retains input while the overlay is
            // hidden.
            if let Some(egui_state) = crate::egui_impl::EguiState::get().as_mut()
                && egui_state.is_input_captured()
            {
                egui_state.toggle_game_input_capture();
            }
            crate::capture::toggle();
        }
        return LRESULT(0);
    }

    if let Some(egui_state) = crate::egui_impl::EguiState::get().as_mut() {
        egui_state.wndproc(hwnd, msg, wparam, lparam);
    }
    WNDPROC.get().unwrap().call(hwnd, msg, wparam, lparam)
}
