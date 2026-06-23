//! Floating-HUD redirect: render the game's HUD into our own offscreen texture.
//!
//! The Scaleform UI normally renders into the engine's working surface, so it composites onto the
//! scene at the screen plane. Step one of the floating HUD is to redirect it into a texture we own, by
//! rebinding the UI's render buffer ([`UIManager::m_RenderBuffer`]) to a render-target view over our
//! texture via [`RenderTargetData::UpdateData`]. Once redirected, the HUD no longer lands on the
//! working surface, so it drops out of the scene composite automatically; we later draw our texture as
//! a 3D quad.
//!
//! The rebind is not tied to startup, so under late injection (where `InitPlatformRT` has already
//! created `m_RenderBuffer`) we just call `UpdateData` once. We install lazily on the render thread and
//! re-apply on a resolution change, the same compare-and-recreate pattern the FSR and debug captures
//! use. Disabling the redirect (or unloading) restores the engine's own binding by re-running
//! [`UIManager::InitPlatformRT`], so the UI never renders into a freed texture.
//!
//! `InitPlatformRT` rebinds GPU views, so the redirect and the restore both run from the per-frame
//! [`tick`] on the render thread. Shutdown (game thread) just clears the config flag and lets a few
//! more frames tick before the hooks come down, so the restore happens on the render thread the same
//! way a toggle-off does.
//!
//! The module is split into the GPU resources ([`target`]), the game-side UI rebind operations
//! ([`binding`]), and the [`state`] machine that drives them.

mod binding;
mod quad;
mod state;
mod target;

pub use state::HUD_STATE;

use jc3gi::graphics_engine::{device::Device, texture::Texture};
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;

/// The per-frame render-thread step: redirects the HUD into our texture while enabled, restores the
/// engine binding while disabled. Called from the render-thread post-draw hook.
pub fn tick(device: &Device, width: u32, height: u32) {
    let mut hud = HUD_STATE.lock();
    if crate::config::Config::lock_query(|c| c.hud.redirect) {
        hud.ensure_redirected(device, width, height);
    } else {
        hud.restore(width);
    }
}

/// Draw the redirected HUD as a floating quad for `eye` over `target` (the eye's linear back buffer),
/// when both the redirect and the quad are enabled. Then clear the HUD render target so the next
/// frame starts clean. Called from the render-thread post-draw hook, before the back buffer is
/// captured/presented, with the engine context mutex held.
pub fn draw_quad(context: &ID3D11DeviceContext, device: &Device, target: &Texture, eye: usize) {
    if crate::config::Config::lock_query(|c| c.hud.redirect && c.hud.quad) {
        let mut hud = HUD_STATE.lock();
        hud.draw_quad(context, device, target, eye);
        hud.clear(context);
    }
}

/// Register the HUD's shutdown cleanup. Call once at init. The cleanup clears the redirect config flag
/// (so the per-frame [`tick`] restores the engine binding on the render thread, as a toggle-off does)
/// and releases the egui preview registration. The shutdown path delays the hook uninstall, giving a
/// few frames to tick the restore through before the hooks come down.
pub fn install() {
    crate::lifecycle::on_cleanup(|renderer| {
        crate::config::CONFIG.lock().hud.redirect = false;
        HUD_STATE.lock().release_preview(renderer);
    });
}
