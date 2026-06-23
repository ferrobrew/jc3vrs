//! Subsystem cleanup registry, so teardown doesn't hardcode each subsystem.
//!
//! A subsystem that needs to undo something on shutdown ([crate::hud]'s HUD-redirect restore, an egui
//! texture registration) registers a cleanup closure with [`on_cleanup`] when it installs. The
//! shutdown path calls [`run_cleanups`] once, which runs them in reverse registration order (last
//! installed, first torn down).
//!
//! Cleanups run on the game thread and receive the egui renderer, so they can release renderer-bound
//! resources directly. Work that must happen on the render thread (GPU rebinds) is done by clearing a
//! config flag the per-frame render hook acts on, then delaying the hook uninstall a few frames so it
//! ticks through (see [`crate::hud::install`] and `shutdown_startup`).

use parking_lot::Mutex;

/// A registered cleanup, run once at shutdown on the game thread with the egui renderer.
type Cleanup = Box<dyn FnOnce(&mut egui_directx11::Renderer) + Send>;

static CLEANUPS: Mutex<Vec<Cleanup>> = Mutex::new(Vec::new());

/// Register `cleanup` to run at shutdown. Cleanups run in reverse registration order.
pub fn on_cleanup(cleanup: impl FnOnce(&mut egui_directx11::Renderer) + Send + 'static) {
    CLEANUPS.lock().push(Box::new(cleanup));
}

/// Run and drain all registered cleanups (reverse order). Call once from the shutdown path, while the
/// hooks are still live so render-thread cleanups can complete.
pub fn run_cleanups(renderer: &mut egui_directx11::Renderer) {
    let cleanups = std::mem::take(&mut *CLEANUPS.lock());
    for cleanup in cleanups.into_iter().rev() {
        cleanup(renderer);
    }
}
