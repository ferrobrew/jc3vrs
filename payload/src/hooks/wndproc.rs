use detours_macro::detour;
use re_utilities::hook_library::HookLibrary;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::{
        Input::KeyboardAndMouse::{VK_F7, VK_F10, VK_F11},
        WindowsAndMessaging::{
            WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_RBUTTONUP,
            WM_SYSKEYDOWN,
        },
    },
};

use crate::hud::cursor;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&WNDPROC_BINDER)
}

#[detour(address = jc3gi::window::WndProc_ADDRESS)]
fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // F7 recenters the headpose: snapshots the current headpose as the neutral reference so
    // subsequent poses are relative to it. Edge-detected like F10/F11.
    if (msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN) && wparam.0 == VK_F7.0 as usize {
        let previous_down = (lparam.0 & 0x4000_0000) != 0;
        if !previous_down {
            crate::headpose::recenter();
            tracing::info!("Headpose recentered");
        }
        return LRESULT(0);
    }

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

    // F11 flips the sun-shadow PCF patch and reloads the shaders, for a live in-headset A/B of the
    // patch without reaching for the debug overlay. Edge-detected like F10; consumed so the game does
    // not also act on it.
    if (msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN) && wparam.0 == VK_F11.0 as usize {
        let previous_down = (lparam.0 & 0x4000_0000) != 0;
        if !previous_down {
            let now_on = {
                let mut cfg = crate::config::CONFIG.lock();
                cfg.stereo.patch_shadow_pcf_hash = !cfg.stereo.patch_shadow_pcf_hash;
                cfg.stereo.patch_shadow_pcf_hash
            };
            crate::hooks::graphics_engine::shader::request_reload();
            tracing::info!(
                "F11: sun-shadow PCF patch {}; reloading shaders to apply",
                if now_on { "ON" } else { "OFF" }
            );
        }
        return LRESULT(0);
    }

    // Track the mouse buttons and wheel for the panel cursor (see `crate::hud::cursor`): the
    // `SendMouseEvents` detour hands Scaleform the live button bitmask each frame instead of the
    // steering action map's effector edges. Observed, not consumed -- egui and the game still see
    // the messages.
    match msg {
        WM_LBUTTONDOWN => cursor::on_button(cursor::Button::Left, true),
        WM_LBUTTONUP => cursor::on_button(cursor::Button::Left, false),
        WM_RBUTTONDOWN => cursor::on_button(cursor::Button::Right, true),
        WM_RBUTTONUP => cursor::on_button(cursor::Button::Right, false),
        // The wheel delta is the signed high word of `wParam`, in `WHEEL_DELTA` units.
        WM_MOUSEWHEEL => cursor::on_wheel(((wparam.0 >> 16) & 0xffff) as u16 as i16 as i32),
        _ => {}
    }

    if let Some(egui_state) = crate::egui_impl::EguiState::get().as_mut() {
        egui_state.wndproc(hwnd, msg, wparam, lparam);
    }
    WNDPROC.get().unwrap().call(hwnd, msg, wparam, lparam)
}
