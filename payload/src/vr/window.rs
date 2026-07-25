//! The game window's Win32 geometry.
//!
//! The HWND is reached through the graphics params rather than being cached, because the engine owns
//! the window and the mod has no creation hook to latch onto. Every read goes to `GetClientRect`, so
//! a WM-driven resize is picked up on the next frame without any notification plumbing.

use jc3gi::graphics_engine::graphics_engine::get_graphics_params;
use windows::Win32::{
    Foundation::{HWND, RECT},
    UI::WindowsAndMessaging::GetClientRect,
};

/// The game window's client size (`width`, `height`) in pixels, or `None` if it cannot be read or is
/// degenerate.
pub(crate) fn client_size() -> Option<(u32, u32)> {
    let (left, top, right, bottom) = rect()?;
    let (w, h) = (right - left, bottom - top);
    (w > 0 && h > 0).then_some((w as u32, h as u32))
}

/// The game window's client rect (`left, top, right, bottom`), or `None` if it cannot be read.
pub(super) fn rect() -> Option<(i32, i32, i32, i32)> {
    let hwnd: HWND = unsafe { get_graphics_params() }.m_Hwnd;
    let mut rect = RECT::default();
    // SAFETY: `hwnd` is the live game window handle; `GetClientRect` errors on a bad handle.
    unsafe { GetClientRect(hwnd, &mut rect) }.ok()?;
    Some((rect.left, rect.top, rect.right, rect.bottom))
}
