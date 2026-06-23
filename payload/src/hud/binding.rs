//! The game-side UI rebinding operations: pointing the Scaleform UI at our target, and restoring the
//! engine's own binding. Both touch the live UI singleton, so they must run on the render thread.

use jc3gi::ui::ui_manager::GetIUIManager;
use windows::core::Interface as _;

use super::target::HudTarget;

/// Rebind the UI's render buffer to render into `target`. Returns whether it took: the UI singleton
/// and its render buffer must be live (they are under late injection, once `InitPlatformRT` has run).
pub(super) fn redirect_to(target: &HudTarget) -> bool {
    // SAFETY: GetIUIManager returns the live UI singleton; m_RenderBuffer is the Scaleform render
    // buffer InitPlatformRT created. UpdateData rebinds its views, refcounting them, so calling it
    // standalone is safe.
    unsafe {
        let Some(manager) = GetIUIManager().as_mut() else {
            return false;
        };
        let Some(render_buffer) = manager.m_RenderBuffer.as_mut() else {
            return false;
        };
        render_buffer.UpdateData(
            target.color_rtv().as_raw(),
            std::ptr::null_mut(),
            target.depth_dsv().as_raw(),
        );
    }
    true
}

/// Restore the engine's own UI binding by re-running `InitPlatformRT`, which rebuilds `m_RenderBuffer`
/// from the engine surface -- the same path the engine takes on a device reset. `width` is the
/// viewport width it expects (the back-buffer width).
pub(super) fn restore_engine_binding(width: u32) {
    // SAFETY: the UI singleton is live; InitPlatformRT rebinds its render buffer to the engine
    // surface, releasing our views.
    unsafe {
        if let Some(manager) = GetIUIManager().as_mut() {
            manager.InitPlatformRT(width as i32);
        }
    }
}
