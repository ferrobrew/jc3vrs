//! Per-eye native render resolution: render each eye at the HMD-recommended resolution.
//!
//! ## Mechanism (deferred state, not a direct `ApplyResize`)
//!
//! The engine sizes every scene render target from `device->m_DeviceInfo.m_DisplayWidth`/
//! `m_DisplayHeight` through `CreateRenderSetups`, re-run at runtime only by `ApplyResize`
//! (`docs/engine/rendering/render-setups-reinit.md`). Rather than call `ApplyResize` directly, this drives the
//! engine's **own deferred display-mode state**: it writes the pending dimensions into
//! [`m_WindowWidth`](GraphicsEngine::m_WindowWidth)/[`m_WindowHeight`](GraphicsEngine::m_WindowHeight)
//! and sets [`m_HasNewWindowSettings`](GraphicsEngine::m_HasNewWindowSettings), exactly as
//! `GraphicsEngine::ResizeBuffers` does for a windowed/settings resize.
//! [`HandleModeChange`](GraphicsEngine::HandleModeChange), serviced once per frame in the `Draw`
//! prologue (which runs inside the first eye's `game.Draw`, see `payload/src/hooks/game.rs`), then
//! calls `ApplyResize(m_WindowWidth, m_WindowHeight)` at the exact frame boundary the engine chose --
//! previous dispatch drained, this frame not yet dispatched -- so the idle-context assumption
//! `ApplyResize` needs holds by construction (`docs/engine/rendering/render-setups-reinit.md` §2/§6). We populate the
//! request from the frame top ([`apply_native_resolution`], before the eye loop) so it is visible to
//! that prologue.
//!
//! Driving the full `ApplyResize`, rather than a scene-only `CreateRenderSetups`, is what makes the
//! *whole* pipeline follow: the pass-owned pools resize through the registered callbacks and the
//! Scaleform view size through the UI reset, neither of which `CreateRenderSetups` touches
//! (`docs/engine/rendering/render-setups-reinit.md` §3/§5). It also sets `CameraManager.m_AspectRatio` from the
//! per-eye `width/height`, so flatscreen-built projections do not render squashed. It **never touches
//! the Win32 window** (§4: it never calls `SetWindowPos`).
//!
//! It would also resize the DXGI swapchain buffers to the same size (`Graphics::ResizeBuffers`).
//! While the mod owns the back buffer ([`crate::vr::back_buffer`], the default in a session) that
//! call is substituted, so the swapchain holds at the window size while everything else follows the
//! render size; this module sequences the ownership flag against the resize that carries it, in both
//! directions. Without ownership the swapchain follows along, and presenting is suppressed in VR
//! (`BLOCK_FLIP`) so the desktop-visible effect is nil either way.
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

use jc3gi::{
    camera::camera_manager::CameraManager, graphics_engine::graphics_engine::GraphicsEngine,
};
use parking_lot::Mutex;

use crate::{config::Config, vr::back_buffer};

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
    // The engine render target size: the per-eye resolution, or 2x its width under single-pass
    // double-wide (both eye-halves side by side). `None` when no session is running or native
    // resolution is off, which drives a restore to the original size.
    let target = if native_enabled {
        crate::vr::engine_render_resolution()
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
            let after = crate::vr::window::rect();
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
            if pending.frames % 60 == 0 {
                tracing::debug!(
                    target: "vr",
                    "native resolution: resize pending, {} frames elapsed",
                    pending.frames,
                );
            }
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
    // The size the engine should be at: the native target, or -- when restoring -- the window's live
    // client size, falling back to the size captured at take-over. The live rect wins because the
    // engine follows WM window resizes itself, so a window moved during the session makes the capture
    // stale, and restoring it would leave the game rendering at the wrong size for its own window.
    // A `None` original with a `None` target means we never took over, so there is nothing to do.
    let desired = target.or_else(|| restore_target(&st));
    let Some(desired) = desired else {
        return;
    };
    // Whether the mod should own the back buffer: only while driving a render size of our own, and
    // only when asked to.
    let want_owned = target.is_some() && Config::lock_query(|c| c.vr.own_back_buffer);
    // An ownership change needs an `ApplyResize` to carry it, because that is what rebuilds the
    // render setups the substitution swaps (and, on release, what puts the engine's own back). So a
    // toggle at an unchanged size still issues a resize -- otherwise flipping `vr.own_back_buffer`
    // mid-session would appear to do nothing until the next resolution change.
    let ownership_change = want_owned != back_buffer::owned();
    if (desired == current && !ownership_change) || st.pending.is_some() {
        return;
    }

    // Ordering is load-bearing in both directions (see `crate::vr::back_buffer`): take ownership
    // *before* the resize that installs the substitution, and release it *before* the resize that
    // rebuilds the engine's own objects, so the engine is never left bound to a texture the mod is
    // about to free.
    if want_owned {
        back_buffer::enable();
    } else {
        back_buffer::disable();
    }

    issue_resize(&mut st, ge, current, desired);
}

/// The lifecycle cleanup: mark shutdown and restore the pre-VR display size **synchronously**.
///
/// Eject renders no further frames (the game thread tears down and unloads without another `Draw`), so
/// the deferred path used during play -- write the pending size, let the engine's `HandleModeChange`
/// service it in the next `Draw` prologue -- would never complete, leaving the game stuck at the mod's
/// (double-wide) render size. Instead call [`ApplyResize`](GraphicsEngine::ApplyResize) directly here:
/// the eject's shader-bundle bounce has already drained the draw thread, and we drain again, so the
/// idle-context precondition holds by construction.
fn on_shutdown() {
    // Release back-buffer ownership *before* the restore, so the `ApplyResize` below runs the stock
    // path: the engine frees the substitutes it allocated and rebuilds its own alias and render
    // setups over the live swapchain. Clearing it afterwards would leave the engine bound to a
    // texture we are about to free (see `crate::vr::back_buffer`).
    back_buffer::disable();

    restore_display_size();

    {
        // A backstop, not the usual path: the `ApplyResize` above normally runs `CreateRenderSetups`,
        // whose epilogue frees the texture at the one moment the engine is known to have stopped
        // using it. This covers the case where the restore took an early exit and no resize happened
        // at all -- and it is safe precisely because the release refuses while a substitution is
        // still installed, rather than because of anything this call site knows.
        // Unconditional: it is a no-op when nothing is held, and gating it on this call having been
        // the one to clear ownership would skip it entirely when the session ended earlier.
        // SAFETY: ownership is cleared; the release checks for itself whether the engine still holds
        // the substitutes.
        if let Some(ge) = unsafe { GraphicsEngine::get() } {
            unsafe { back_buffer::release_backing_texture(ge) };
        }
    }
}

/// The size to restore the engine to when the mod stops driving it: the window's live client size,
/// falling back to the size captured at take-over.
///
/// The live rect wins because the engine follows WM window resizes itself, so a window moved during
/// the session makes the capture stale, and restoring it would leave the game rendering at the wrong
/// size for its own window. Shared by the per-frame restore and the synchronous shutdown restore --
/// the two ran different expressions once, disagreed, and the later one dragged the game back to the
/// stale size.
fn restore_target(st: &ResolutionState) -> Option<(u32, u32)> {
    st.original
        .map(|original| crate::vr::window::client_size().unwrap_or(original))
}

/// Restore the engine's display size to the pre-VR value, synchronously. Split out of [`on_shutdown`]
/// so its early exits cannot skip the back-buffer release that must follow it.
fn restore_display_size() {
    let mut st = STATE.lock();
    st.shutting_down = true;
    let Some(original) = st.original else {
        return;
    };
    // Restore to the window's *current* client size rather than the size captured at bring-up (see
    // the note in `apply_native_resolution`). Recorded back into `original` so that if a later frame
    // still runs the per-frame restore, it agrees rather than dragging the game back to the stale
    // size -- which is exactly what undid this restore before.
    let original = restore_target(&st).unwrap_or(original);
    st.original = Some(original);
    st.pending = None; // supersede any in-flight deferred resize; we restore directly below

    // SAFETY: game thread during eject; every hop is null-guarded.
    let Some(ge) = (unsafe { GraphicsEngine::get() }) else {
        // No engine to resize means no way to rebuild its aliases either: if a substitution is still
        // installed, it is about to become permanent (see `back_buffer::release_backing_texture`).
        if back_buffer::installed() {
            tracing::error!(
                target: "vr",
                "native resolution: shutdown restore found no graphics engine while a back-buffer \
                 substitution is installed; the engine's aliases will stay pointed at the mod's \
                 texture and the desktop view will stay dead until the engine resizes on its own",
            );
        }
        return;
    };
    if !ge.m_HasBeenInitialized {
        if back_buffer::installed() {
            tracing::error!(
                target: "vr",
                "native resolution: shutdown restore found an uninitialized graphics engine while a \
                 back-buffer substitution is installed; the engine's aliases will stay pointed at the \
                 mod's texture and the desktop view will stay dead until the engine resizes on its own",
            );
        }
        return;
    }
    let current = {
        let Some(device) = (unsafe { ge.m_Device.as_ref() }) else {
            if back_buffer::installed() {
                tracing::error!(
                    target: "vr",
                    "native resolution: shutdown restore found no graphics device while a back-buffer \
                     substitution is installed; the engine's aliases will stay pointed at the mod's \
                     texture and the desktop view will stay dead until the engine resizes on its own",
                );
            }
            return;
        };
        (
            device.m_DeviceInfo.m_DisplayWidth,
            device.m_DeviceInfo.m_DisplayHeight,
        )
    };
    // A substitution installed means the engine's `m_BackBufferLinear`/render setups still point at
    // the mod's texture; only an `ApplyResize` rebuilds them (`CreateRenderSetups`' epilogue is what
    // clears `INSTALLED`). So the resize below is forced even when the size already matches `original`
    // -- it is not being issued to change the size, but to make the engine rebuild its own aliases and
    // hand the swapchain back. Skipping it because "nothing to resize" is exactly what leaves the
    // substitution installed after the DLL unloads.
    let force_for_installed_substitution = back_buffer::installed();
    if current == original && !force_for_installed_substitution {
        return;
    }

    tracing::info!(
        target: "vr",
        from_width = current.0,
        from_height = current.1,
        to_width = original.0,
        to_height = original.1,
        forced_for_installed_substitution = force_for_installed_substitution,
        "native resolution: synchronous shutdown restore (eject renders no further frames)",
    );
    // Clear the engine's deferred mode-change request too, so a stray frame cannot re-apply the mod's
    // size after we restore. SAFETY: as above; `ApplyResize` runs on the drained idle context.
    ge.m_WindowWidth = original.0;
    ge.m_WindowHeight = original.1;
    ge.m_HasNewWindowSettings = false;
    unsafe {
        ge.WaitForCPUDrawToFinish();
        ge.ApplyResize(original.0, original.1);
    }
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
    let window_before = crate::vr::window::rect();

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
