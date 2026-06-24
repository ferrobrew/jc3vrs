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
//! The floating panel lazily follows the head's orientation with deadzones and critically-damped
//! easing ([`HUD_STATE`].update_follow). The panel's world-space corners are computed once per
//! frame (eye 0) and projected through each eye's own per-eye VP, so it sits at a finite world
//! position with correct stereo depth rather than being head-locked at infinity.
//!
//! The module is split into the GPU resources ([`target`]), the game-side UI rebind operations
//! ([`binding`]), the quad draw pass ([`quad`]), the [`state`] machine that drives them, and the
//! [`config`] types for tuning parameters.

mod binding;
mod config;
mod quad;
mod state;
mod target;

pub use config::HudConfig;
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
/// when both the redirect and the quad are enabled. On eye 0, updates the lazy-follow damping from
/// the current camera orientation and computes the panel's world-space corners (cached for eye 1).
/// Then draws and clears. Called from the render-thread post-draw hook, before the back buffer is
/// captured/presented, with the engine context mutex held.
pub fn draw_quad(context: &ID3D11DeviceContext, device: &Device, target: &Texture, eye: usize) {
    let cfg = crate::config::Config::lock_query(|c| c.hud);
    if !cfg.redirect || !cfg.quad {
        return;
    }

    let mut hud = HUD_STATE.lock();

    // Compute world-space corners once per frame on eye 0 and cache them. Both eyes then
    // project the same world-space quad through their own per-eye VP, producing correct stereo
    // disparity. Computing corners per-eye instead would cancel the world transform against the
    // per-eye view (VP = Inverse(Transform) · Projection), collapsing the panel to view space
    // (head-locked, zero disparity, appears at infinity).
    if eye == 0 {
        let (head_yaw, head_pitch, head_roll) = extract_head_orientation();
        let (follow_yaw, follow_pitch, follow_roll) = hud.update_follow(
            &state::FollowParams {
                head_yaw,
                head_pitch,
                head_roll,
            },
            &cfg.follow,
        );
        hud.compute_world_corners(&quad::PanelParams {
            width: u32::from(target.m_Width),
            height: u32::from(target.m_Height),
            distance: cfg.distance,
            panel_height: cfg.panel_height,
            follow_yaw,
            follow_pitch,
            follow_roll,
        });
    }

    hud.draw_quad(context, device, target, eye);
    hud.clear(context);
}

/// Extract yaw, pitch, and roll (in degrees) from the render camera's world transform.
/// Returns `(0.0, 0.0, 0.0)` if the camera is not available.
fn extract_head_orientation() -> (f32, f32, f32) {
    let transform = unsafe {
        let cm = jc3gi::camera::camera_manager::CameraManager::get();
        let cam = cm.and_then(|cm| cm.m_RenderCamera.as_ref());
        cam.map(|cam| cam.m_TransformF.data)
    };
    let Some(transform) = transform else {
        return (0.0, 0.0, 0.0);
    };

    // The camera's world transform has its basis vectors in the rows (pyxis docs):
    // data[0..2] = right (+X), data[4..6] = up (+Y), data[8..10] = +Z basis (forward = -Z basis).
    let forward_x = -transform[8];
    let forward_y = -transform[9];
    let forward_z = -transform[10];

    // Yaw: angle from -Z around the Y axis. 0 when looking along -Z, positive when turning right.
    let yaw = forward_x.atan2(-forward_z).to_degrees();
    let horizontal_len = (forward_x * forward_x + forward_z * forward_z).sqrt();
    let pitch = if horizontal_len > 1e-6 {
        forward_y.atan2(horizontal_len).to_degrees()
    } else {
        90.0 * forward_y.signum()
    };

    // Roll: the right vector's tilt from horizontal. Positive = right side down (clockwise from
    // behind, matching the yaw convention).
    let right_xz_len = (transform[0] * transform[0] + transform[2] * transform[2]).sqrt();
    let roll = (-transform[1]).atan2(right_xz_len).to_degrees();

    (yaw, pitch, roll)
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
