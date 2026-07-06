//! The virtual mouse cursor for panel UI interaction (issue #9).
//!
//! In stereo the OS cursor is invisible -- the player sees two rendered views, and the desktop
//! cursor lands in neither -- so navigating the game's menus with the mouse needs a cursor of our
//! own, rendered onto the floating panel.
//!
//! The game's own mouse path is `WndProc` (`WM_MOUSEMOVE`) -> `CUIManager::SetMousePos` ->
//! `CUIManager::SendMouseEvents` -> `GFx::MovieImpl::HandleEvent`, converting window-client pixels
//! to movie-viewport pixels by subtracting the centering offset of the movie rectangle inside the
//! device viewport. The HUD redirect breaks that conversion: it points the cached viewport and the
//! movie rectangle at our offscreen texture (with its own dimensions and aspect), so window-client
//! pixels no longer relate to either. The `SendMouseEvents` detour (see [`crate::hooks::ui`])
//! replaces the conversion outright: the window-client position is normalized against the window
//! size and rescaled to the movie rectangle (= our texture), then fed to the movie via
//! `NotifyMouseState`, whose per-frame position-plus-buttons snapshot also sidesteps the original's
//! "only move when the DirectInput mouse reported a delta" gate.
//!
//! Because the mapping normalizes both axes independently, a window aspect different from the
//! panel aspect stretches mouse travel rather than letterboxing it: every point on the panel stays
//! reachable, at the cost of slightly anisotropic cursor speed.
//!
//! This module is the shared state between the writers -- the `WndProc` detour (buttons, wheel),
//! the render-thread [`tick`](crate::hud::tick) (frame geometry), and the `SendMouseEvents` detour
//! (position, visibility) -- and the reader, the panel draw, which renders the cursor as a small
//! circle-dot quad lifted slightly off the panel toward the camera (see
//! [`quad`](super::quad) and [`state`](super::state)).

use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use parking_lot::Mutex;

/// A mouse button, indexed the way Scaleform's button bitmask expects.
#[derive(Clone, Copy)]
pub enum Button {
    Left = 0,
    Right = 1,
}

/// The cursor's position on the panel texture (normalized UV, `(0, 0)` top-left) and whether the
/// game's cursor policy shows it this frame.
#[derive(Clone, Copy)]
pub struct CursorFrame {
    pub u: f32,
    pub v: f32,
}

/// Report a mouse-button transition from the `WndProc` detour. Tracked as a live bitmask so the
/// `SendMouseEvents` detour can hand Scaleform the full button state each frame.
pub fn on_button(button: Button, down: bool) {
    let bit = 1u32 << (button as u32);
    if down {
        BUTTONS.fetch_or(bit, Ordering::Relaxed);
    } else {
        BUTTONS.fetch_and(!bit, Ordering::Relaxed);
    }
}

/// Accumulate a `WM_MOUSEWHEEL` delta (in `WHEEL_DELTA` units) from the `WndProc` detour.
pub fn on_wheel(delta: i32) {
    WHEEL.fetch_add(delta, Ordering::Relaxed);
}

/// The live mouse-button bitmask (bit 0 left, bit 1 right), the format
/// `MovieImpl::NotifyMouseState` expects.
pub fn buttons() -> u32 {
    BUTTONS.load(Ordering::Relaxed)
}

/// Drain the accumulated wheel movement into Flash line units (the engine's own conversion is
/// `delta / WHEEL_DELTA * 3`). Zero when the wheel has not moved.
pub fn take_wheel_lines() -> f32 {
    let raw = WHEEL.swap(0, Ordering::Relaxed);
    raw as f32 / 120.0 * 3.0
}

/// Publish the frame's coordinate-mapping geometry from the render-thread HUD tick: the game's
/// back-buffer size (the fallback normalization space), and the redirected texture's size (`None`
/// while the redirect is not applied, which disables the injection).
pub fn set_geometry(window: (u32, u32), movie: Option<(u32, u32)>) {
    let mut state = STATE.lock();
    state.window = window;
    state.movie = movie;
}

/// Publish the game window's live client-rect size from the `WndProc` detour. The mouse position
/// is in window-client pixels, so this -- not the back-buffer size -- is the correct
/// normalization space: under Wine/Proton fullscreen scaling (or any window/back-buffer size
/// divergence, e.g. an HMD-shaped swap chain behind a desktop-shaped window) the two differ, and
/// normalizing against the back buffer skews the axis whose sizes disagree.
pub fn set_client_size(size: (u32, u32)) {
    STATE.lock().client = Some(size);
}

/// The frame's mapping geometry `(window size, movie size)`, or `None` while the redirect is not
/// applied or a dimension is degenerate. The window size is the live client rect when one has
/// been seen, else the back-buffer size.
pub fn geometry() -> Option<((u32, u32), (u32, u32))> {
    let state = STATE.lock();
    let movie = state.movie?;
    let window = state.client.unwrap_or(state.window);
    if window.0 == 0 || window.1 == 0 || movie.0 == 0 || movie.1 == 0 {
        return None;
    }
    Some((window, movie))
}

/// Publish the cursor's panel position for the renderer, or `None` to hide it (gamepad in use,
/// egui capturing input, the game's cursor policy hiding it, or the injection inactive).
pub fn set_frame(frame: Option<CursorFrame>) {
    STATE.lock().frame = frame;
}

/// The cursor's panel position, when it should be drawn this frame.
pub fn frame() -> Option<CursorFrame> {
    STATE.lock().frame
}

static BUTTONS: AtomicU32 = AtomicU32::new(0);
static WHEEL: AtomicI32 = AtomicI32::new(0);
static STATE: Mutex<State> = Mutex::new(State {
    window: (0, 0),
    client: None,
    movie: None,
    frame: None,
});

/// The cross-thread cursor state: geometry written on the render thread, the client rect from the
/// `WndProc` detour, the frame written from the `SendMouseEvents` detour (window or game thread),
/// all read wherever needed. Locked only for these short copies.
struct State {
    window: (u32, u32),
    client: Option<(u32, u32)>,
    movie: Option<(u32, u32)>,
    frame: Option<CursorFrame>,
}
