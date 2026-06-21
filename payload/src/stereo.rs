//! Live stereo runtime state and the per-eye gating helpers shared across the render hooks.
//!
//! The Draw driver (`hooks::core::game`) maintains [`StereoState`]: it sets `active` from
//! `config.stereo.enabled` at the start of a frame and bumps `draw_index` per eye dispatch. Hooks
//! read it via [`is_second_eye`] / [`draw_index`] / [`active`]. This is *runtime* state,
//! distinct from the `stereo` config toggles in [`crate::config`].

use parking_lot::Mutex;

/// The live stereo render state for the frame in flight.
pub struct StereoState {
    /// Whether the current frame is being rendered in stereo (the Draw driver double-Draws).
    pub active: bool,
    /// The eye currently being drawn: 0 = first, 1 = second.
    pub draw_index: usize,
}
impl StereoState {
    const fn new() -> Self {
        Self {
            active: false,
            draw_index: 0,
        }
    }
}

/// Global live stereo state, written by the Draw driver and read by the render hooks.
pub static STEREO_STATE: Mutex<StereoState> = Mutex::new(StereoState::new());

/// Whether the current frame is being rendered in stereo.
pub fn active() -> bool {
    STEREO_STATE.lock().active
}

/// The eye currently being drawn (0 = first, 1 = second).
pub fn draw_index() -> usize {
    STEREO_STATE.lock().draw_index
}

/// True while the Draw driver is rendering the *second* eye of a stereo frame.
pub fn is_second_eye() -> bool {
    let state = STEREO_STATE.lock();
    state.active && state.draw_index == 1
}
