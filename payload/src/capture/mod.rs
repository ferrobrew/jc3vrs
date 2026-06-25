//! The F10 fullscreen stereo capture mode: a separate borderless window + swapchain that composites
//! both eyes side by side for clean recording, hiding the debug UI overlay.
//!
//! The capture window is created lazily on the first active frame and destroyed on toggle-off or
//! unmount. Each frame (from the stereo Draw driver in `hooks::game`), if active, the capture state
//! composites the two eye captures (from `EGUI_DEBUG_RENDER_STATE`) into the capture swapchain's
//! back buffer and presents it. The game's own window, swapchain, and present path are untouched.
//!
//! Threading: everything runs on the game's main thread -- the same thread that owns the game's
//! HWND and runs `graphics_flip` and the stereo eye loop. The capture window's HWND is created on
//! that thread, the swapchain is presented from it, and `DestroyWindow` is called from it. The
//! composite draws use the engine's immediate context under `Context::m_Mutex`, serialized with the
//! engine's own render work.

use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Context as _;
use parking_lot::Mutex;
use windows::{
    Win32::Graphics::Direct3D11::{ID3D11ShaderResourceView, ID3D11Texture2D},
    core::Interface,
};

use crate::ui::render::EGUI_DEBUG_RENDER_STATE;

use composite::CaptureComposite;
use window::CaptureWindow;

mod composite;
mod window;

/// Whether the capture mode is currently active (F10 toggled on). Read by the render hooks to
/// suppress the egui overlay and by the Draw driver to drive the per-frame composite.
static CAPTURE_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Set by the capture window's `WM_DESTROY` handler, so the state machine can detect external
/// destruction and rebuild. Uses an atomic (not the `CAPTURE_STATE` mutex) because `DestroyWindow`
/// fires `WM_DESTROY` synchronously on the same thread, which would re-enter the lock -- a
/// `parking_lot::Mutex` is not reentrant.
static WINDOW_DESTROYED: AtomicBool = AtomicBool::new(false);

/// The live capture state. Locked briefly on the main thread during the per-frame present.
static CAPTURE_STATE: Mutex<CaptureState> = Mutex::new(CaptureState::new());

/// A cached SRV over an eye capture texture, keyed on the texture's raw COM pointer so the SRV is
/// recreated when `EGUI_DEBUG_RENDER_STATE` rebuilds the underlying texture on a resize.
struct CachedEyeSrv {
    texture_ptr: usize,
    srv: ID3D11ShaderResourceView,
}

struct CaptureState {
    window: Option<CaptureWindow>,
    composite: Option<CaptureComposite>,
    /// Per-eye SRV cache. `None` when the eye texture is unavailable or the SRV failed to build.
    eye_srvs: [Option<CachedEyeSrv>; 2],
}

impl CaptureState {
    const fn new() -> Self {
        Self {
            window: None,
            composite: None,
            eye_srvs: [const { None }, const { None }],
        }
    }

    /// Drop everything but the composite pipeline (which is size-independent and cheap to keep).
    /// The window's `destroy` must run on the owning thread.
    fn teardown_window(&mut self) {
        if let Some(window) = self.window.take() {
            window.destroy();
        }
        self.eye_srvs = [None, None];
    }
}

/// Toggle the capture mode (F10). The actual window create/destroy is deferred to the next
/// `present_frame` on the main thread, so this is safe to call from the WndProc hook.
pub fn toggle() {
    let new = !CAPTURE_ACTIVE.load(Ordering::Relaxed);
    tracing::info!(target: "capture", "toggle -> active={new}");
    set_active(new);
}

/// Set the capture mode on or off. If turning on, the next `present_frame` lazily creates the
/// window; if turning off, it tears down.
pub fn set_active(active: bool) {
    CAPTURE_ACTIVE.store(active, Ordering::Relaxed);
}

/// Whether the capture mode is currently active.
pub fn is_active() -> bool {
    CAPTURE_ACTIVE.load(Ordering::Relaxed)
}

/// Called from the capture window's `WM_DESTROY` handler, so the state machine can detect external
/// destruction (e.g. the system tore down the window) and rebuild rather than drawing to a dead
/// HWND.
pub(super) fn on_window_destroyed() {
    WINDOW_DESTROYED.store(true, Ordering::Relaxed);
}

/// The per-frame step: drive the capture state machine (create/destroy/composite+present). Called
/// once per real frame from the stereo Draw driver (`hooks::game`), after both eyes have been
/// captured. Safe to call when capture is inactive -- it tears down the window if one is still live.
pub fn present_frame() {
    if !is_active() {
        let mut state = CAPTURE_STATE.lock();
        let had_window = state.window.is_some();
        state.teardown_window();
        state.composite = None;
        WINDOW_DESTROYED.store(false, Ordering::Relaxed);
        if had_window {
            tracing::info!(target: "capture", "torn down (was inactive)");
        }
        return;
    }

    // Active: ensure resources, composite, present. Errors tear down and disable capture so a
    // half-built state doesn't persist across frames.
    let result = unsafe { present_active() };
    if let Err(e) = result {
        tracing::error!(target: "capture", "present_active failed: {e:#}");
        let mut state = CAPTURE_STATE.lock();
        state.teardown_window();
        state.composite = None;
        CAPTURE_ACTIVE.store(false, Ordering::Relaxed);
        tracing::info!(target: "capture", "disabled capture after error");
    }
}

/// The active-frame path. Fetches the device + context, ensures the window/composite/SRVs exist,
/// composites both eyes, and presents.
///
/// # Safety
/// Reads engine singletons via raw pointer dereference; the graphics engine, device, and context
/// must be live (they are while the hooks are installed and a frame is in flight).
unsafe fn present_active() -> anyhow::Result<()> {
    let ge = unsafe { jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() }
        .context("the graphics engine is unavailable")?;
    let device = unsafe { ge.m_Device.as_ref() }.context("the graphics device is unavailable")?;
    let context =
        unsafe { device.m_Context.as_ref() }.context("the graphics context is unavailable")?;

    // Eye size = back buffer size. The capture swapchain is `2 * eye_width x eye_height`, one half
    // per eye. Read before locking CAPTURE_STATE so the (brief) EGUI_DEBUG_RENDER_STATE lock taken
    // below is not nested under CAPTURE_STATE.
    let eye_size = unsafe {
        device
            .m_BackBuffer
            .as_ref()
            .map(|bb| (u32::from(bb.m_Width), u32::from(bb.m_Height)))
    }
    .context("the back buffer is unavailable")?;

    // Clone the eye textures (AddRef) under a brief EGUI_DEBUG_RENDER_STATE lock, dropped before the
    // CAPTURE_STATE lock is taken -- avoids nesting the two mutexes.
    let eye_textures: [Option<ID3D11Texture2D>; 2] = {
        let lock = EGUI_DEBUG_RENDER_STATE.lock();
        [lock.texture(0).cloned(), lock.texture(1).cloned()]
    };

    let mut state = CAPTURE_STATE.lock();

    // If the window was destroyed externally, drop the stale CaptureWindow (its COM refs) and
    // rebuild. The HWND is already gone, so we do NOT call `destroy` here.
    if WINDOW_DESTROYED.swap(false, Ordering::Relaxed) {
        state.window = None;
        state.eye_srvs = [None, None];
    }

    // (Re)create the window if missing or the eye size changed.
    let want_size = (eye_size.0 * 2, eye_size.1);
    let need_recreate = state
        .window
        .as_ref()
        .map(|w| w.size() != want_size)
        .unwrap_or(true);
    if need_recreate {
        state.teardown_window();
        state.window = Some(CaptureWindow::new(device, eye_size.0, eye_size.1)?);
    }

    // Build the composite pipeline lazily (it is size-independent, so it survives a resize).
    if state.composite.is_none() {
        state.composite = Some(CaptureComposite::new(device)?);
    }

    // Resolve per-eye SRVs first (mutable borrow of `state.eye_srvs`), before borrowing the window
    // and composite immutably for the draw. The returned SRVs are owned clones, so the mutable
    // borrows end here.
    let eye0_srv = resolve_eye_srv(&mut state.eye_srvs[0], eye_textures[0].as_ref(), device);
    let eye1_srv = resolve_eye_srv(&mut state.eye_srvs[1], eye_textures[1].as_ref(), device);
    let cross_eyed = crate::ui::render::STEREO_CROSS_EYED.load(Ordering::Relaxed);

    let window = state.window.as_ref().expect("window was just ensured");
    let composite = state
        .composite
        .as_ref()
        .expect("composite was just ensured");
    let back_size = window.size();

    // Composite + present under the engine context mutex, serialized with the render work.
    unsafe {
        windows::Win32::System::Threading::EnterCriticalSection(context.m_Mutex);
        composite.draw(
            &context.m_Context,
            window.rtv(),
            back_size,
            eye0_srv.as_ref(),
            eye1_srv.as_ref(),
            cross_eyed,
        );
        windows::Win32::System::Threading::LeaveCriticalSection(context.m_Mutex);
    }

    window
        .present(1)
        .context("IDXGISwapChain::Present failed")?;
    Ok(())
}

/// Get the SRV for `eye_texture`, creating and caching it if the cached SRV was built for a
/// different texture (or none yet). Device methods are free-threaded, so this is safe under the
/// CAPTURE_STATE lock.
fn resolve_eye_srv(
    cache: &mut Option<CachedEyeSrv>,
    eye_texture: Option<&ID3D11Texture2D>,
    device: &jc3gi::graphics_engine::device::Device,
) -> Option<ID3D11ShaderResourceView> {
    let tex = eye_texture?;
    let ptr = tex.as_raw() as usize;
    if cache.as_ref().is_some_and(|c| c.texture_ptr == ptr) {
        return cache.as_ref().map(|c| c.srv.clone());
    }

    let mut srv: Option<ID3D11ShaderResourceView> = None;
    let result = unsafe {
        device
            .m_Device
            .CreateShaderResourceView(tex, None, Some(&mut srv))
    };
    if result.is_err() || srv.is_none() {
        tracing::warn!("capture: failed to create an SRV for an eye texture");
        return None;
    }
    let srv = srv.expect("the SRV was created");
    *cache = Some(CachedEyeSrv {
        texture_ptr: ptr,
        srv: srv.clone(),
    });
    Some(srv)
}

/// Register the capture module's shutdown cleanup. Call once at init. The cleanup disables capture
/// mode and tears down the window + swapchain so the game isn't left in a broken state on unmount.
pub fn install() {
    crate::lifecycle::on_cleanup(|_renderer| {
        set_active(false);
        let mut state = CAPTURE_STATE.lock();
        state.teardown_window();
        state.composite = None;
        WINDOW_DESTROYED.store(false, Ordering::Relaxed);
    });
}
