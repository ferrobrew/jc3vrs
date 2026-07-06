//! Per-eye native render resolution: render each eye at the HMD-recommended resolution.
//!
//! ## Mechanism (deferred state, not a direct `ApplyResize`)
//!
//! The engine sizes every scene render target from `device->m_DeviceInfo.m_DisplayWidth`/
//! `m_DisplayHeight` through `CreateRenderSetups`, re-run at runtime only by `ApplyResize`
//! (`docs/engine/render-setups-reinit.md`). Rather than call `ApplyResize` directly, this drives the
//! engine's **own deferred display-mode state**: it writes the pending dimensions into
//! [`m_WindowWidth`](GraphicsEngine::m_WindowWidth)/[`m_WindowHeight`](GraphicsEngine::m_WindowHeight)
//! and sets [`m_HasNewWindowSettings`](GraphicsEngine::m_HasNewWindowSettings), exactly as
//! `GraphicsEngine::ResizeBuffers` does for a windowed/settings resize.
//! [`HandleModeChange`](GraphicsEngine::HandleModeChange), serviced once per frame in the `Draw`
//! prologue (which runs inside the first eye's `game.Draw`, see `payload/src/hooks/game.rs`), then
//! calls `ApplyResize(m_WindowWidth, m_WindowHeight)` at the exact frame boundary the engine chose --
//! previous dispatch drained, this frame not yet dispatched -- so the idle-context assumption
//! `ApplyResize` needs holds by construction (`docs/engine/render-setups-reinit.md` §2/§6). We populate the
//! request from the frame top ([`apply_native_resolution`], before the eye loop) so it is visible to
//! that prologue.
//!
//! Driving the full `ApplyResize` (as opposed to a scene-only `CreateRenderSetups`) also resizes the
//! DXGI swapchain buffers to the same size (`Graphics::ResizeBuffers`), which **never touches the
//! Win32 window** (`docs/engine/render-setups-reinit.md` §4/§7: it never calls `SetWindowPos`). Presenting is
//! suppressed in VR (`BLOCK_FLIP`), so the desktop-visible effect is nil, and this keeps
//! `m_BackBufferLinear` and the back-buffer render setups coherent with the per-eye scene targets for
//! free -- the low-risk path. `ApplyResize` also sets `CameraManager.m_AspectRatio` from the per-eye
//! `width/height`, so flatscreen-built projections do not render squashed. The per-eye capture
//! textures follow the back-buffer size automatically (`payload/src/ui/render.rs`).
//!
//! ## Restore
//!
//! The pre-VR display size is captured before the first resize. When the session ends (loss,
//! `vr.enabled` off) the per-frame tick requests a resize back to it; on uninject a registered
//! lifecycle cleanup ([`on_shutdown`]) sets the same deferred restore while the hooks are still live,
//! so the delayed hook uninstall (`lib.rs` `shutdown_startup`) leaves the `Draw` prologue time to
//! service it and the game is left exactly as found.
//!
//! ## Failure handling
//!
//! If the request is not serviced within [`SERVICE_TIMEOUT_FRAMES`], comes back at the wrong size, or
//! the Win32 window rect changes across the resize, native resolution is disabled at runtime
//! (`vr.native_resolution = false`, logged) and the original size is restored; the mod continues at
//! desktop resolution. Never crashes, never wedges.

use parking_lot::Mutex;
use windows::Win32::{
    Foundation::{HWND, RECT},
    UI::WindowsAndMessaging::GetClientRect,
};

use jc3gi::{
    camera::camera_manager::CameraManager,
    graphics_engine::graphics_engine::{GraphicsEngine, get_graphics_params},
};

use crate::config::Config;

/// How many frames to wait for a requested deferred resize to be serviced before treating it as a
/// fault. A resize is serviced in the very next `Draw` prologue, so this is a generous ceiling that
/// only trips on a genuinely stuck or faulted resize.
const SERVICE_TIMEOUT_FRAMES: u32 = 240;

/// The native-resolution driver state, on the game thread. A const-constructible [`Mutex`] singleton.
static STATE: Mutex<ResolutionState> = Mutex::new(ResolutionState::new());

/// Register the shutdown restore. Called once from [`crate::vr::install`].
pub fn install() {
    crate::lifecycle::on_cleanup(|_renderer| on_shutdown());
}

/// The once-per-frame driver, called from the frame top in `hooks::game::game_update_render` **before
/// the eye loop**, so the request is visible to the first eye's `Draw` prologue that services it.
///
/// Requests a deferred engine resize to the per-eye native size while a session is running and
/// `vr.native_resolution` is on, and back to the captured pre-VR size otherwise. Verifies each
/// serviced resize (size and window rect) and disables native resolution on any fault. A no-op until
/// the engine is initialized, and cheap when the size is already correct.
pub fn apply_native_resolution() {
    let native_enabled = Config::lock_query(|c| c.vr.native_resolution);
    // The per-eye target, matching the swapchain; `None` when no session is running or native
    // resolution is off, which drives a restore to the original size.
    let target = if native_enabled {
        super::native_eye_resolution()
    } else {
        None
    };

    // SAFETY: the graphics-engine singleton and its device are live once the engine is initialized;
    // every hop is null-guarded. `m_Device.as_ref()` on the raw pointer field does not borrow `ge`.
    let Some(ge) = (unsafe { GraphicsEngine::get() }) else {
        return;
    };
    if !ge.m_HasBeenInitialized {
        return;
    }
    let Some(device) = (unsafe { ge.m_Device.as_ref() }) else {
        return;
    };
    let current = (
        device.m_DeviceInfo.m_DisplayWidth,
        device.m_DeviceInfo.m_DisplayHeight,
    );

    let mut st = STATE.lock();

    // Service an in-flight request before issuing a new one.
    if let Some(mut pending) = st.pending.take() {
        if current == pending.target {
            let after = window_rect();
            let window_ok = match (pending.window_before, after) {
                (Some(before), Some(after)) => before == after,
                // A missing rect on either side cannot prove a change; do not fault on it.
                _ => true,
            };
            let aspect = camera_aspect_ratio();
            tracing::info!(
                target: "vr",
                width = current.0,
                height = current.1,
                restore = pending.is_restore,
                aspect_ratio = aspect,
                "native resolution: engine resize serviced",
            );
            if !window_ok {
                tracing::error!(
                    target: "vr",
                    before = ?pending.window_before,
                    after = ?after,
                    "native resolution: the Win32 window rect changed across the resize (expected untouched); disabling",
                );
                if !pending.is_restore {
                    disable_native_resolution();
                }
            }
            // Pending consumed; fall through to (re)compute the desired size, which will now request
            // a restore if the fault above disabled native resolution.
        } else {
            pending.frames += 1;
            if pending.frames > SERVICE_TIMEOUT_FRAMES {
                tracing::error!(
                    target: "vr",
                    requested_width = pending.target.0,
                    requested_height = pending.target.1,
                    resulting_width = current.0,
                    resulting_height = current.1,
                    "native resolution: resize was not serviced (faulted); disabling and restoring",
                );
                if !pending.is_restore {
                    disable_native_resolution();
                }
                // Pending consumed; fall through to request the restore.
            } else {
                // Still waiting; keep the request in flight and do nothing else this frame.
                st.pending = Some(pending);
                return;
            }
        }
    }

    // Recompute the target after a possible in-fault disable (which cleared `vr.native_resolution`).
    // Once shutdown has requested the restore, never re-request native.
    let target = if st.shutting_down || !Config::lock_query(|c| c.vr.native_resolution) {
        None
    } else {
        target
    };
    // The size the engine should be at: the native target, or the captured original when restoring.
    // A `None` original with a `None` target means we never took over, so there is nothing to do.
    let desired = target.or(st.original);
    let Some(desired) = desired else {
        return;
    };
    if desired == current || st.pending.is_some() {
        return;
    }

    issue_resize(&mut st, ge, current, desired);
}

/// The lifecycle cleanup: mark shutdown and request a restore to the pre-VR display size while the
/// hooks are still live, so the delayed uninstall services it (`lib.rs` `shutdown_startup`).
fn on_shutdown() {
    let mut st = STATE.lock();
    st.shutting_down = true;
    let Some(original) = st.original else {
        return;
    };
    if st.pending.is_some() {
        // A resize is already in flight; the still-live tick will settle it toward the original.
        return;
    }
    // SAFETY: as in `apply_native_resolution`; every hop is null-guarded.
    let Some(ge) = (unsafe { GraphicsEngine::get() }) else {
        return;
    };
    if !ge.m_HasBeenInitialized {
        return;
    }
    let Some(device) = (unsafe { ge.m_Device.as_ref() }) else {
        return;
    };
    let current = (
        device.m_DeviceInfo.m_DisplayWidth,
        device.m_DeviceInfo.m_DisplayHeight,
    );
    if current == original {
        return;
    }
    tracing::info!(target: "vr", "native resolution: shutdown restore to the pre-VR display size");
    issue_resize(&mut st, ge, current, original);
}

/// Populate the engine's deferred display-mode state so its next `Draw` prologue `HandleModeChange`
/// applies the resize, capturing the pre-resize state for verification and restore.
fn issue_resize(
    st: &mut ResolutionState,
    ge: &mut GraphicsEngine,
    current: (u32, u32),
    target: (u32, u32),
) {
    // Capture the pre-VR display size before the first resize, for restore.
    if st.original.is_none() {
        st.original = Some(current);
    }
    let is_restore = st.original == Some(target);
    let window_before = window_rect();

    // The same fields `GraphicsEngine::ResizeBuffers` stashes for a deferred windowed/settings resize.
    ge.m_WindowWidth = target.0;
    ge.m_WindowHeight = target.1;
    ge.m_HasNewWindowSettings = true;

    tracing::info!(
        target: "vr",
        from_width = current.0,
        from_height = current.1,
        to_width = target.0,
        to_height = target.1,
        restore = is_restore,
        "native resolution: requesting deferred engine resize",
    );

    st.pending = Some(Pending {
        target,
        is_restore,
        window_before,
        frames: 0,
    });
}

/// Disable native resolution at runtime after a fault, so the next tick restores the original size and
/// the mod continues at desktop resolution.
fn disable_native_resolution() {
    crate::config::CONFIG.lock().vr.native_resolution = false;
}

/// The game window's client rect (`left, top, right, bottom`), or `None` if it cannot be read.
fn window_rect() -> Option<(i32, i32, i32, i32)> {
    let hwnd: HWND = unsafe { get_graphics_params() }.m_Hwnd;
    let mut rect = RECT::default();
    // SAFETY: `hwnd` is the live game window handle; `GetClientRect` returns an error for a bad handle.
    unsafe { GetClientRect(hwnd, &mut rect) }.ok()?;
    Some((rect.left, rect.top, rect.right, rect.bottom))
}

/// The engine camera manager's aspect ratio, for the serviced-resize log (set by `ApplyResize`).
fn camera_aspect_ratio() -> Option<f32> {
    // SAFETY: the camera-manager singleton is live once the engine is running; null-guarded by `get`.
    unsafe { CameraManager::get() }.map(|cm| cm.m_AspectRatio)
}

/// The native-resolution driver state.
struct ResolutionState {
    /// The pre-VR display size, captured before the first resize; the target for a restore. `None`
    /// until we have taken over the resolution at least once.
    original: Option<(u32, u32)>,
    /// The in-flight deferred resize, awaiting service by the engine's `HandleModeChange`.
    pending: Option<Pending>,
    /// Set once the shutdown restore has been requested, so nothing re-requests native afterward.
    shutting_down: bool,
}

impl ResolutionState {
    const fn new() -> Self {
        Self {
            original: None,
            pending: None,
            shutting_down: false,
        }
    }
}

/// An in-flight deferred resize request.
struct Pending {
    /// The requested display size; the resize is serviced once the device reports this size.
    target: (u32, u32),
    /// Whether this request restores the original size (so a window-rect change is not attributed to
    /// the native path, and does not re-disable an already-disabled feature).
    is_restore: bool,
    /// The window client rect at request time, compared after service to confirm the window is
    /// untouched. `None` if it could not be read.
    window_before: Option<(i32, i32, i32, i32)>,
    /// Frames waited for service, for the timeout fault.
    frames: u32,
}
