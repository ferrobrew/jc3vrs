//! The HUD-redirect state machine: it lazily creates the target, applies and relinquishes the rebind,
//! owns the egui preview registration, and drives the lazy-follow damping for the floating panel.

use std::time::Instant;

use glam::{Quat, Vec3};
use jc3gi::graphics_engine::{device::Device, texture::Texture};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CLEAR_DEPTH, D3D11_CLEAR_STENCIL, ID3D11DeviceContext,
};

use super::{HudMode, binding, quad::HudQuad, split, target::HudTarget};

/// Global HUD state. Locked briefly on the render thread.
pub static HUD_STATE: Mutex<HudState> = Mutex::new(HudState::new());

/// The live HUD-redirect state (render thread only, apart from the preview registration on the UI
/// thread).
pub struct HudState {
    target: Option<HudTarget>,
    /// Whether the redirect is currently applied to the UI's render buffer.
    redirected: bool,
    /// The last back-buffer size seen by [`ensure_redirected`]. A change means the engine re-ran
    /// `InitPlatformRT` (its device/reset path), rebuilding `m_RenderBuffer` from the engine surface
    /// and discarding our rebind -- so the redirect must be re-applied even though our target texture
    /// is independent of the back buffer.
    back_buffer_size: Option<(u32, u32)>,
    /// The egui texture id for the HUD preview, registered lazily on the UI thread.
    preview_id: Option<egui::TextureId>,
    /// The quad pass that draws the redirected HUD back into the scene, built lazily on first draw.
    quad: Option<HudQuad>,
    /// Lazy-follow damping state for the floating panel (gameplay HUD mode).
    follow: FollowState,
    /// The latched world-static pose `(position, rotation)` for [`HudMode::Movie`](crate::hud::HudMode::Movie):
    /// captured when the mode is entered and held so the panel does not move with the head. `None`
    /// outside movie mode.
    latched_pose: Option<(Vec3, Quat)>,
    /// The panel pose `(position, rotation)` chosen for the current frame, cached for the marker
    /// projection ([`compute_panel_vp`](crate::hud::compute_panel_vp)) to reuse.
    current_pose: Option<(Vec3, Quat)>,
    /// The extra layer targets for the multi-pass split ([`HudLayer::Markers`](split::HudLayer)
    /// and [`HudLayer::Center`](split::HudLayer)); layer 0 (static) renders into
    /// [`target`](HudState::target). Present only while the split is enabled and sized like the
    /// main target.
    layer_targets: [Option<HudTarget>; split::LAYER_COUNT - 1],
    /// World-space panel corners, computed once per frame on eye 0 and reused for eye 1 so both
    /// eyes project the same world position through their own per-eye VP (correct stereo depth).
    cached_corners: Option<[[f32; 4]; 4]>,
    /// While split: per-layer world-space corners and their distances, computed alongside
    /// [`cached_corners`](HudState::cached_corners) on eye 0. Index matches
    /// [`split::LAYERS`]; layer 0 (static) reuses `cached_corners`' geometry but is duplicated
    /// here so the draw can sort all layers by distance.
    cached_layer_corners: [Option<([[f32; 4]; 4], f32)>; split::LAYER_COUNT],
}

/// Damped follow state: tracks the panel's eased orientation via quaternion slerp.
struct FollowState {
    /// Damped panel rotation (quaternion).
    rotation: Quat,
    /// Last frame's instant for delta-time computation.
    last_time: Option<Instant>,
}

impl FollowState {
    const fn new() -> Self {
        Self {
            rotation: Quat::IDENTITY,
            last_time: None,
        }
    }

    /// Ease the panel's rotation toward the head's current rotation using critically-damped
    /// slerp: `alpha = 1 - 2^(-dt/halflife); current = slerp(current, target, alpha)`. No deadzone
    /// -- the panel always follows, with the halflife controlling the lag. Returns the damped
    /// quaternion for the quad's world-space orientation.
    fn update(&mut self, head_rotation: Quat, config: &super::config::FollowConfig) -> Quat {
        let dt = self
            .last_time
            .map(|t| t.elapsed().as_secs_f32())
            .unwrap_or(0.016);
        self.last_time = Some(Instant::now());
        let dt = dt.min(0.1);

        let alpha = (1.0 - 2.0_f32.powf(-dt / config.rotation_halflife)).min(1.0);
        self.rotation = self.rotation.slerp(head_rotation, alpha);

        self.rotation
    }
}

impl HudState {
    const fn new() -> Self {
        Self {
            target: None,
            redirected: false,
            back_buffer_size: None,
            preview_id: None,
            quad: None,
            follow: FollowState::new(),
            latched_pose: None,
            current_pose: None,
            cached_corners: None,
            cached_layer_corners: [None, None, None],
            layer_targets: [None, None],
        }
    }

    /// Ensure the HUD is redirected into a target at `texture_width` x `texture_height`, (re)creating
    /// the target on a config-size change and applying the rebind once. A back-buffer size change
    /// (`back_buffer_width`/`back_buffer_height`) forces a re-apply without recreating the target,
    /// because the engine re-runs `InitPlatformRT` on a device/reset and discards our rebind. A failed
    /// target build or a not-yet-live UI leaves the state unredirected, so the next tick retries.
    pub(super) fn ensure_redirected(
        &mut self,
        device: &Device,
        texture_width: u32,
        texture_height: u32,
        back_buffer_width: u32,
        back_buffer_height: u32,
    ) {
        if texture_width == 0 || texture_height == 0 {
            return;
        }

        // Recreate the target when the configured HUD resolution changes (or when none exists yet).
        if self.target.as_ref().map(HudTarget::size) != Some((texture_width, texture_height)) {
            match HudTarget::new(device, texture_width, texture_height) {
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

        // A back-buffer size change means the engine re-ran `InitPlatformRT` (its device/reset path),
        // which rebuilds `m_RenderBuffer` from the engine surface and discards our rebind. Re-apply it
        // even though our target texture is independent of the back buffer.
        if self.back_buffer_size != Some((back_buffer_width, back_buffer_height)) {
            self.back_buffer_size = Some((back_buffer_width, back_buffer_height));
            self.redirected = false;
        }

        let Some(target) = self.target.as_ref() else {
            return;
        };
        if !self.redirected && binding::redirect_to(target, texture_width, texture_height) {
            self.redirected = true;
        }
    }

    /// Ensure the extra layer targets exist at the main target's size while the split is enabled,
    /// and drop them while it is not. Layer creation failures log and leave the slot empty, which
    /// keeps the split inactive (see [`split_views`](HudState::split_views)).
    pub(super) fn ensure_layers(&mut self, device: &Device, enabled: bool) {
        if !enabled {
            self.layer_targets = [None, None];
            return;
        }
        let Some((width, height)) = self.target.as_ref().map(HudTarget::size) else {
            return;
        };
        for slot in &mut self.layer_targets {
            if slot.as_ref().map(HudTarget::size) != Some((width, height)) {
                match HudTarget::new(device, width, height) {
                    Ok(target) => *slot = Some(target),
                    Err(e) => {
                        tracing::error!("HUD split layer: {e:#}");
                        *slot = None;
                        return;
                    }
                }
            }
        }
    }

    /// The per-layer render-target views for the split passes, or `None` unless the redirect is
    /// applied and every layer target exists. The views are cloned (COM-refcounted), so they stay
    /// valid for the frame even if the targets are recreated concurrently.
    pub fn split_views(&self) -> Option<split::LayerViews> {
        if !self.redirected {
            return None;
        }
        let main = self.target.as_ref()?;
        let markers = self.layer_targets[0].as_ref()?;
        let center = self.layer_targets[1].as_ref()?;
        Some(split::LayerViews {
            views: [main, markers, center].map(|t| (t.color_rtv().clone(), t.depth_dsv().clone())),
        })
    }

    /// Restore the engine's own binding and drop our target, so the UI no longer renders into our
    /// texture. A no-op when not redirected. `back_buffer_width`/`back_buffer_height` are the
    /// back-buffer dimensions `InitPlatformRT` and the viewport expect.
    pub(super) fn restore(&mut self, back_buffer_width: u32, back_buffer_height: u32) {
        if self.redirected {
            binding::restore_engine_binding(back_buffer_width, back_buffer_height);
            self.redirected = false;
        }
        self.target = None;
    }

    /// Choose the panel pose `(position, rotation)` for the current frame from the head pose and the
    /// [`HudMode`](crate::hud::HudMode), caching it for the marker projection to reuse.
    ///
    /// In [`HudMode::Hud`](crate::hud::HudMode::Hud) the panel tracks the head: the position is the
    /// live head position and the rotation is the damped follow quaternion.
    /// [`HudMode::Movie`](crate::hud::HudMode::Movie) is world-static -- the pose is latched on the
    /// first movie-mode frame and held, so head movement no longer moves it (the latch is cleared
    /// whenever `Hud` mode resumes) -- but only while
    /// [`WORLD_STATIC_MOVIE_PANEL`](crate::hud::WORLD_STATIC_MOVIE_PANEL) is enabled. While it is
    /// disabled the panel head-follows in both modes (see that constant for why).
    pub fn update_pose(
        &mut self,
        mode: HudMode,
        head_pos: Vec3,
        head_rotation: Quat,
        follow: &super::config::FollowConfig,
    ) -> (Vec3, Quat) {
        let world_static = super::WORLD_STATIC_MOVIE_PANEL && mode == HudMode::Movie;
        let pose = if world_static {
            *self.latched_pose.get_or_insert((head_pos, head_rotation))
        } else {
            self.latched_pose = None;
            (head_pos, self.follow.update(head_rotation, follow))
        };
        self.current_pose = Some(pose);
        pose
    }

    /// The panel pose chosen for the current frame. Used by the W2S hook to project markers onto the
    /// panel's surface rather than the screen plane. `None` until [`update_pose`](HudState::update_pose)
    /// has run.
    pub fn panel_pose(&self) -> Option<(Vec3, Quat)> {
        self.current_pose
    }

    /// Compute the panel's world-space corners from the current camera and follow state, caching
    /// the result for both eyes. Call once per frame (eye 0); eye 1 reuses the cached corners.
    pub fn compute_world_corners(&mut self, params: &super::quad::PanelParams) {
        self.cached_corners = None;
        if let Some(corners) = super::quad::compute_world_corners(params) {
            self.cached_corners = Some(corners);
        }
    }

    /// Compute the split layers' world-space corners (one set per layer, each at its own
    /// distance), cached for both eyes like [`compute_world_corners`](HudState::compute_world_corners).
    /// Pass `None` to clear (split off this frame).
    pub fn compute_layer_corners(
        &mut self,
        params: Option<[super::quad::PanelParams; split::LAYER_COUNT]>,
    ) {
        self.cached_layer_corners = [None, None, None];
        let Some(params) = params else {
            return;
        };
        for (slot, params) in self.cached_layer_corners.iter_mut().zip(params.iter()) {
            if let Some(corners) = super::quad::compute_world_corners(params) {
                *slot = Some((corners, params.distance));
            }
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
            // While split, composite the extra layers over the static panel with the same
            // world-space corners (bottom to top: markers, then the center/reticle group). The
            // per-layer depth arrives with the depth-composite step; identical corners keep the
            // split visually equivalent to the single-texture panel until then.
            for layer in self.layer_targets.iter().flatten() {
                quad.draw(
                    context,
                    device,
                    target,
                    &layer.color_srv().clone(),
                    &corners,
                );
            }
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
        // SAFETY: `context` is the live engine context; the RTVs and DSVs belong to our target
        // textures.
        unsafe {
            for target in std::iter::once(target).chain(self.layer_targets.iter().flatten()) {
                context.ClearRenderTargetView(target.color_rtv(), &[0.0, 0.0, 0.0, 0.0]);
                context.ClearDepthStencilView(
                    target.depth_dsv(),
                    (D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL).0,
                    1.0,
                    0,
                );
            }
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
