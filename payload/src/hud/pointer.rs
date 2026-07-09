//! The panel pointer: turns the desktop mouse into a UV on the floating panel, and (for the
//! interactive egui panel, issue #24) into egui pointer events.
//!
//! The MVP source is the desktop mouse: the window client-pixel position (tracked by the `WndProc`
//! detour into [`super::cursor`]) normalized against the window and fed to egui as if it were the
//! pointer, so the panel is driven by the same mouse the flat overlay uses. This is deliberately a
//! narrow seam: a future VR controller-ray source would add a ray-vs-panel-plane intersection here
//! (reusing [`super::quad::compute_world_corners`]'s corner layout, UV `(0, 0)` = the texture's
//! top-left) and produce the same `(u, v)` the mouse path produces, leaving everything downstream
//! unchanged.
//!
//! Only the pointer *position* is re-sourced here (and, in panel mode, the button and wheel state,
//! since their window-pixel coordinates would otherwise land in the wrong space); keyboard, text, and
//! paste still flow through [`crate::egui_impl::EguiState::wndproc`] unchanged.

use std::sync::atomic::{AtomicU32, Ordering};

use super::cursor;

/// The panel-texture UV of the desktop mouse this frame, or `None` when no mouse position or window
/// size is known yet. Shared by the egui event pump and the panel's own cursor dot so they agree.
pub fn window_uv() -> Option<(f32, f32)> {
    let (mx, my) = cursor::mouse_pos()?;
    let (nw, nh) = cursor::normalization_size()?;
    let u = (mx as f32 / nw as f32).clamp(0.0, 1.0);
    let v = (my as f32 / nh as f32).clamp(0.0, 1.0);
    Some((u, v))
}

/// Build the egui pointer events for the VR panel from the desktop mouse: a move to the current UV,
/// press/release for any button that changed since the last call, and any accumulated wheel motion.
/// Positions are `uv * panel_size` so they land in the panel texture's coordinate space (the same
/// space egui lays out in while [`crate::egui_impl::EguiState::set_panel_mode`] is active). Call once
/// per frame from the game thread before [`crate::egui_impl::EguiState::run`].
pub fn window_mouse_events(panel_size: (u32, u32)) -> Vec<egui::Event> {
    let mut events = Vec::new();
    let Some((u, v)) = window_uv() else {
        // Still drain the wheel so it does not burst on the next frame a position is known.
        let _ = cursor::take_wheel_notches();
        PREV_BUTTONS.store(cursor::buttons(), Ordering::Relaxed);
        return events;
    };
    let pos = egui::Pos2::new(u * panel_size.0 as f32, v * panel_size.1 as f32);
    events.push(egui::Event::PointerMoved(pos));

    let buttons = cursor::buttons();
    let prev = PREV_BUTTONS.swap(buttons, Ordering::Relaxed);
    let changed = buttons ^ prev;
    for (bit, button) in [
        (
            1u32 << (cursor::Button::Left as u32),
            egui::PointerButton::Primary,
        ),
        (
            1u32 << (cursor::Button::Right as u32),
            egui::PointerButton::Secondary,
        ),
    ] {
        if changed & bit != 0 {
            events.push(egui::Event::PointerButton {
                pos,
                button,
                pressed: buttons & bit != 0,
                modifiers: egui::Modifiers::default(),
            });
        }
    }

    let notches = cursor::take_wheel_notches();
    if notches != 0.0 {
        events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::Vec2::new(0.0, notches),
            modifiers: egui::Modifiers::default(),
        });
    }

    events
}

/// The previous frame's button bitmask (same layout as [`cursor::buttons`]), for edge detection.
static PREV_BUTTONS: AtomicU32 = AtomicU32::new(0);
