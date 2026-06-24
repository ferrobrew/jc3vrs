//! The HUD-redirect state machine: it lazily creates the target, applies and relinquishes the rebind,
//! owns the egui preview registration, and drives the lazy-follow damping for the floating panel.

use std::time::Instant;

use jc3gi::graphics_engine::{device::Device, texture::Texture};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CLEAR_DEPTH, D3D11_CLEAR_STENCIL, ID3D11DeviceContext,
};

use super::{binding, quad::HudQuad, target::HudTarget};

/// Parameters for the lazy-follow damping update.
pub(crate) struct FollowParams {
    pub head_yaw: f32,
    pub head_pitch: f32,
}

/// Global HUD state. Locked briefly on the render thread.
pub static HUD_STATE: Mutex<HudState> = Mutex::new(HudState::new());

/// The live HUD-redirect state (render thread only, apart from the preview registration on the UI
/// thread).
pub struct HudState {
    target: Option<HudTarget>,
    /// Whether the redirect is currently applied to the UI's render buffer.
    redirected: bool,
    /// The egui texture id for the HUD preview, registered lazily on the UI thread.
    preview_id: Option<egui::TextureId>,
    /// The quad pass that draws the redirected HUD back into the scene, built lazily on first draw.
    quad: Option<HudQuad>,
    /// Lazy-follow damping state for the floating panel.
    follow: FollowState,
    /// World-space panel corners, computed once per frame on eye 0 and reused for eye 1 so both
    /// eyes project the same world position through their own per-eye VP (correct stereo depth).
    cached_corners: Option<[[f32; 4]; 4]>,
}

/// Damped follow state: tracks the panel's eased yaw and pitch offset relative to the head.
struct FollowState {
    /// Damped yaw offset in radians (positive = panel rotated right of head center).
    yaw: f32,
    /// Damped pitch offset in radians (positive = panel rotated above head center).
    pitch: f32,
    /// Last frame's instant for delta-time computation.
    last_time: Option<Instant>,
}

impl FollowState {
    const fn new() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.0,
            last_time: None,
        }
    }

    /// Update the damped offsets toward the head's orientation, using the critically-damped
    /// exponential: `alpha = 1 - 2^(-dt/halflife); current = lerp(current, target, alpha)`.
    /// Returns the resulting damped offsets `(yaw_rad, pitch_rad)` for the quad's world-space
    /// orientation.
    fn update(
        &mut self,
        params: &FollowParams,
        config: &super::config::FollowConfig,
    ) -> (f32, f32) {
        let dt = self
            .last_time
            .map(|t| t.elapsed().as_secs_f32())
            .unwrap_or(0.016);
        self.last_time = Some(Instant::now());
        // Clamp dt to avoid huge leaps on frame drops.
        let dt = dt.min(0.1);

        // Deadzone is relative to the panel's current orientation, not the initial forward.
        // The panel only moves when the head is more than deadzone away from the panel's
        // current yaw/pitch, keeping the panel within a few degrees of the head at all times.
        let yaw_rad = params.head_yaw.to_radians();
        let pitch_rad = params.head_pitch.to_radians();
        let dz_yaw = config.yaw_deadzone.to_radians();
        let dz_pitch = config.pitch_deadzone.to_radians();

        let yaw_delta = shortest_angle_delta(self.yaw, yaw_rad);
        let target_yaw = if yaw_delta.abs() < dz_yaw {
            self.yaw
        } else {
            yaw_rad - dz_yaw * yaw_delta.signum()
        };
        let pitch_delta = pitch_rad - self.pitch;
        let target_pitch = if pitch_delta.abs() < dz_pitch {
            self.pitch
        } else {
            pitch_rad - dz_pitch * pitch_delta.signum()
        };

        // Compute frame-rate independent damping factors.
        let alpha_yaw = (1.0 - 2.0_f32.powf(-dt / config.yaw_halflife)).min(1.0);
        let alpha_pitch = (1.0 - 2.0_f32.powf(-dt / config.pitch_halflife)).min(1.0);

        // Lerp toward targets.
        self.yaw = self.yaw + (target_yaw - self.yaw) * alpha_yaw;
        self.pitch = self.pitch + (target_pitch - self.pitch) * alpha_pitch;

        (self.yaw, self.pitch)
    }
}

/// Shortest angular distance from `from` to `to`, wrapped to [-π, π]. Handles the ±180°
/// discontinuity in yaw so the panel doesn't swing the long way around when the head crosses
/// that boundary.
fn shortest_angle_delta(from: f32, to: f32) -> f32 {
    let pi = std::f32::consts::PI;
    let two_pi = 2.0 * pi;
    let mut delta = (to - from) % two_pi;
    if delta > pi {
        delta -= two_pi;
    } else if delta < -pi {
        delta += two_pi;
    }
    delta
}

impl HudState {
    const fn new() -> Self {
        Self {
            target: None,
            redirected: false,
            preview_id: None,
            quad: None,
            follow: FollowState::new(),
            cached_corners: None,
        }
    }

    /// Ensure the HUD is redirected into our target at `width` x `height`, (re)creating the target on a
    /// size change and applying the rebind once. A failed target build or a not-yet-live UI leaves the
    /// state unredirected, so the next tick retries.
    pub(super) fn ensure_redirected(&mut self, device: &Device, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }

        if self.target.as_ref().map(HudTarget::size) != Some((width, height)) {
            match HudTarget::new(device, width, height) {
                Ok(target) => {
                    self.target = Some(target);
                    self.redirected = false;
                    // The preview SRV belonged to the old texture; re-register it on the next preview.
                    self.preview_id = None;
                }
                Err(e) => {
                    tracing::error!("HUD: {e:#}");
                    self.target = None;
                    return;
                }
            }
        }

        let Some(target) = self.target.as_ref() else {
            return;
        };
        if !self.redirected && binding::redirect_to(target) {
            self.redirected = true;
        }
    }

    /// Restore the engine's own binding and drop our target, so the UI no longer renders into our
    /// texture. A no-op when not redirected. `width` is the back-buffer width `InitPlatformRT` expects.
    pub(super) fn restore(&mut self, width: u32) {
        if self.redirected {
            binding::restore_engine_binding(width);
            self.redirected = false;
        }
        self.target = None;
    }

    /// Update the lazy-follow damping state from the current head orientation. Returns the damped
    /// offsets `(yaw_rad, pitch_rad)` for the quad's 3D rotation.
    pub fn update_follow(
        &mut self,
        params: &FollowParams,
        config: &super::config::FollowConfig,
    ) -> (f32, f32) {
        self.follow.update(params, config)
    }

    /// Compute the panel's world-space corners from the current camera and follow state, caching
    /// the result for both eyes. Call once per frame (eye 0); eye 1 reuses the cached corners.
    pub fn compute_world_corners(
        &mut self,
        width: u32,
        height: u32,
        distance: f32,
        panel_height: f32,
        follow_yaw: f32,
        follow_pitch: f32,
    ) {
        // Invalidate the cache first; if the computation fails, eye 1 won't draw stale corners.
        self.cached_corners = None;
        if let Some(corners) = super::quad::compute_world_corners(
            width,
            height,
            distance,
            panel_height,
            follow_yaw,
            follow_pitch,
        ) {
            self.cached_corners = Some(corners);
        }
    }

    /// Draw the redirected HUD as a floating quad for `eye` over `target` (the eye's linear back
    /// buffer). A no-op when not redirected or when no cached corners are available (e.g. the camera
    /// was unavailable on eye 0). The quad pass is built lazily on first draw. The caller must hold
    /// the engine context mutex.
    pub fn draw_quad(
        &mut self,
        context: &ID3D11DeviceContext,
        device: &Device,
        target: &Texture,
        _eye: usize,
    ) {
        if !self.redirected {
            return;
        }
        let Some(corners) = self.cached_corners else {
            return;
        };
        let Some(hud_srv) = self.target.as_ref().map(|t| t.color_srv().clone()) else {
            return;
        };
        if self.quad.is_none() {
            match HudQuad::new(device) {
                Ok(quad) => self.quad = Some(quad),
                Err(e) => {
                    tracing::error!("HUD quad: {e:#}");
                    return;
                }
            }
        }
        if let Some(quad) = self.quad.as_ref() {
            quad.draw(context, device, target, &hud_srv, &corners);
        }
    }

    /// Clear the HUD render target and depth-stencil so the next frame starts clean. A no-op when
    /// not redirected. The caller must hold the engine context mutex.
    pub fn clear(&mut self, context: &ID3D11DeviceContext) {
        if !self.redirected {
            return;
        }
        let Some(target) = self.target.as_ref() else {
            return;
        };
        // SAFETY: `context` is the live engine context; RTV and DSV belong to our target texture.
        unsafe {
            context.ClearRenderTargetView(target.color_rtv(), &[0.0, 0.0, 0.0, 0.0]);
            context.ClearDepthStencilView(
                target.depth_dsv(),
                (D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL).0,
                1.0,
                0,
            );
        }
    }

    /// Register (once) and return the egui texture id for previewing the redirected HUD. `None` until
    /// the HUD has been redirected into our texture.
    pub fn preview_id(
        &mut self,
        renderer: &mut egui_directx11::Renderer,
    ) -> Option<egui::TextureId> {
        if self.preview_id.is_none()
            && let Some(target) = self.target.as_ref()
        {
            self.preview_id = Some(renderer.register_user_texture(target.color_srv().clone()));
        }
        self.preview_id
    }

    /// Drop the egui preview registration. Call on the UI thread (it owns the renderer), so the texture
    /// id is released rather than leaked.
    pub fn release_preview(&mut self, renderer: &mut egui_directx11::Renderer) {
        if let Some(id) = self.preview_id.take() {
            renderer.unregister_user_texture(id);
        }
    }
}
