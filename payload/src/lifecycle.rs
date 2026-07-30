//! Subsystem cleanup registry, so teardown doesn't hardcode each subsystem.
//!
//! A subsystem that needs to undo something on shutdown ([crate::hud]'s HUD-redirect restore, an egui
//! texture registration) registers a cleanup closure with [`on_cleanup`] when it installs. The
//! shutdown path calls [`run_cleanups`] once, which runs them in reverse registration order (last
//! installed, first torn down).
//!
//! Cleanups run on the game thread and receive the egui renderer *if one exists*, so they can release
//! renderer-bound resources directly. The renderer is optional because a session may eject without
//! ever having brought the debug UI up, and the cleanups that undo engine-wide state -- the
//! double-wide render resolution above all -- must still run. Work that must happen on the render
//! thread (GPU rebinds) is done by clearing a config flag the per-frame render hook acts on, then
//! delaying the hook uninstall a few frames so it ticks through (see [`crate::hud::install`] and
//! `shutdown_startup`).

use parking_lot::Mutex;

/// A registered cleanup, run once at shutdown on the game thread with the egui renderer if there is
/// one.
type Cleanup = Box<dyn FnOnce(Option<&mut egui_directx11::Renderer>) + Send>;

static CLEANUPS: Mutex<Vec<Cleanup>> = Mutex::new(Vec::new());

/// Register `cleanup` to run at shutdown. Cleanups run in reverse registration order.
pub fn on_cleanup(cleanup: impl FnOnce(Option<&mut egui_directx11::Renderer>) + Send + 'static) {
    CLEANUPS.lock().push(Box::new(cleanup));
}

/// Run and drain all registered cleanups (reverse order). Call once from the shutdown path,
/// unconditionally and while the hooks are still live so render-thread cleanups can complete.
pub fn run_cleanups(renderer: Option<&mut egui_directx11::Renderer>) {
    let cleanups = std::mem::take(&mut *CLEANUPS.lock());
    let mut renderer = renderer;
    for cleanup in cleanups.into_iter().rev() {
        cleanup(renderer.as_deref_mut());
    }
}
