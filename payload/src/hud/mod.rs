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
//! The floating panel lazily follows the head's orientation with critically-damped quaternion
//! slerp ([`HUD_STATE`].update_follow). The panel's world-space corners are computed once per
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

use glam::{Mat3, Mat4, Quat, Vec3};
use jc3gi::{
    camera::camera_manager::CameraManager,
    graphics_engine::{device::Device, texture::Texture},
    types::math::Matrix4,
};
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
        let head_rotation = extract_head_rotation();
        let follow_rotation = hud.update_follow(head_rotation, &cfg.follow);
        hud.compute_world_corners(&quad::PanelParams {
            width: u32::from(target.m_Width),
            height: u32::from(target.m_Height),
            distance: cfg.distance,
            panel_height: cfg.panel_height,
            follow_rotation,
        });
    }

    hud.draw_quad(context, device, target, eye);
    hud.clear(context);
}

/// Extract the head's world-space rotation as a quaternion from the render camera's world
/// transform. Returns `Quat::IDENTITY` if the camera is not available.
///
/// The camera's world transform stores its basis vectors in rows (pyxis docs):
/// `data[0..2]` = right (+X), `data[4..6]` = up (+Y), `data[8..10]` = +Z basis (back).
/// We build a `Mat3` from these columns and convert to a quaternion. The quaternion maps
/// camera-local to world space, so `quat * Vec3::NEG_Z` yields the forward direction.
fn extract_head_rotation() -> Quat {
    let transform = unsafe {
        let cm = jc3gi::camera::camera_manager::CameraManager::get();
        let cam = cm.and_then(|cm| cm.m_RenderCamera.as_ref());
        cam.map(|cam| cam.m_TransformF.data)
    };
    let Some(transform) = transform else {
        return Quat::IDENTITY;
    };

    let right = Vec3::new(transform[0], transform[1], transform[2]);
    let up = Vec3::new(transform[4], transform[5], transform[6]);
    let back = Vec3::new(transform[8], transform[9], transform[10]);

    Quat::from_mat3(&Mat3::from_cols(right, up, back))
}

/// Compute the view-projection and camera matrices for the floating panel's orientation, so that
/// W2S (`Get2DInfo`) projects world points onto the panel's surface rather than the screen plane.
/// Returns `(vp, camera_transform)` in engine format, or `None` if the camera is unavailable.
///
/// The panel VP uses the damped follow rotation for orientation and the active camera's position
/// and projection. This ensures markers are positioned correctly on the floating quad: a POI
/// directly ahead of the camera but off-center from the panel's facing direction appears at the
/// correct position on the panel surface, compensating for the follow-damping lag.
pub fn compute_panel_vp() -> Option<(Matrix4, Matrix4)> {
    let follow_rotation = HUD_STATE.lock().follow_rotation();

    let (transform, projection) = unsafe {
        let cm = CameraManager::get()?;
        let active = cm.m_ActiveCamera.as_ref()?;
        (&active.m_TransformF, &active.m_ProjectionF)
    };

    let cam_pos = Vec3::new(transform.data[12], transform.data[13], transform.data[14]);

    // Build the panel world transform from the follow rotation's basis vectors + camera position.
    // The engine stores +Z as the third basis (back), so back = quat * Z (not -Z).
    let right = follow_rotation * Vec3::X;
    let up = follow_rotation * Vec3::Y;
    let back = follow_rotation * Vec3::Z;

    let panel_transform = Mat4::from_cols(
        right.extend(0.0),
        up.extend(0.0),
        back.extend(0.0),
        cam_pos.extend(1.0),
    );

    // View = inverse(world transform). VP = P * V (glam column-vector convention).
    // The Matrix4 ↔ Mat4 From impls transpose, so the engine-format result is correct.
    let panel_view = panel_transform.inverse();
    let glam_proj = Mat4::from(*projection);
    let glam_vp = glam_proj * panel_view;

    let engine_vp = Matrix4::from(glam_vp);
    let engine_camera = Matrix4::from(panel_transform);

    Some((engine_vp, engine_camera))
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
