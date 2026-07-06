//! The HUD-redirect state machine: it lazily creates the target, applies and relinquishes the rebind,
//! owns the egui preview registration, and drives the lazy-follow damping for the floating panel.

use std::time::Instant;

use glam::{Quat, Vec3};
use jc3gi::graphics_engine::{device::Device, texture::Texture};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CLEAR_DEPTH, D3D11_CLEAR_STENCIL, ID3D11DeviceContext,
};

use super::{
    HudMode, binding,
    markers::MarkerDepth,
    quad::HudQuad,
    split,
    target::HudTarget,
    warp::{HudWarp, WarpInputs},
};

/// The per-frame inputs for the panel warp, chosen on eye 0.
pub struct WarpFrame {
    /// The panel anchor (head position) the corners were built around.
    pub anchor: [f32; 3],
    /// The frame's recorded on-screen markers (plus, in single-panel mode, the center bubble as
    /// the first entry).
    pub markers: Vec<MarkerDepth>,
    /// The flat (base) distance of the surface being warped: the panel distance in single-panel
    /// mode, the marker layer's distance under the split.
    pub base_distance: f32,
}

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
    /// The egui texture ids for the marker/center layer previews, registered lazily.
    layer_preview_ids: [Option<egui::TextureId>; split::LAYER_COUNT - 1],
    /// The quad pass that draws the redirected HUD back into the scene, built lazily on first draw.
    quad: Option<HudQuad>,
    /// The marker-layer warp pass, built lazily on the first warped draw.
    warp: Option<HudWarp>,
    /// The dynamic-distance depth sampler, built lazily on the first enabled frame.
    depth_shift: Option<super::depth::DepthShift>,
    /// The frame's warp inputs (eye 0), or `None` when the warp is off this frame.
    warp_frame: Option<WarpFrame>,
    /// Lazy-follow damping state for the floating panel (gameplay HUD mode).
    follow: FollowState,
    /// The latched world-static pose `(position, rotation)` for [`HudMode::Movie`](crate::hud::HudMode::Movie):
    /// captured when the mode is entered and held so the panel does not move with the head. `None`
    /// outside movie mode.
    latched_pose: Option<(Vec3, Quat)>,
    /// The panel pose `(position, rotation)` chosen for the current frame, cached for the marker
    /// projection ([`compute_panel_vp`](crate::hud::compute_panel_vp)) to reuse.
    current_pose: Option<(Vec3, Quat)>,
    /// The extra layer targets for the split ([`HudLayer::Markers`](split::HudLayer)
    /// and [`HudLayer::Center`](split::HudLayer)); layer 0 (static) renders into
    /// [`target`](HudState::target). Present only while the split is enabled and sized like the
    /// main target.
    layer_targets: [Option<HudTarget>; split::LAYER_COUNT - 1],
    /// World-space panel corners, computed once per frame on eye 0 and reused for eye 1 so both
    /// eyes project the same world position through their own per-eye VP (correct stereo depth).
    cached_corners: Option<[[f32; 4]; 4]>,
    /// While split: per-layer world-space corners and their distances, chosen on eye 0. Index
    /// matches [`split::LAYERS`]. The static and center entries are recomputed every frame
    /// (head-locked); the marker entry is frozen at each marker-texture refresh so the stale
    /// texture stays glued to the world pose it was rendered for.
    cached_layer_corners: [Option<([[f32; 4]; 4], f32)>; split::LAYER_COUNT],
    /// Whether the split composite is live this frame (chosen on eye 0; also gates the clear).
    split_composite: bool,
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
            layer_preview_ids: [None, None],
            quad: None,
            warp: None,
            depth_shift: None,
            warp_frame: None,
            follow: FollowState::new(),
            latched_pose: None,
            current_pose: None,
            cached_corners: None,
            cached_layer_corners: [None, None, None],
            split_composite: false,
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
            if self.layer_targets.iter().any(Option::is_some) {
                self.layer_targets = [None, None];
                // The preview SRVs belonged to the dropped textures; re-register next preview.
                self.layer_preview_ids = [None, None];
            }
            return;
        }
        let Some((width, height)) = self.target.as_ref().map(HudTarget::size) else {
            return;
        };
        for slot in &mut self.layer_targets {
            if slot.as_ref().map(HudTarget::size) != Some((width, height)) {
                match HudTarget::new(device, width, height) {
                    Ok(target) => {
                        *slot = Some(target);
                        self.layer_preview_ids = [None, None];
                    }
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
            sizes: [main, markers, center].map(HudTarget::size),
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

    /// The frame's dynamic panel distance: dispatch the depth-histogram pass, pick up the async
    /// readback, and ease toward the policy's target (see [`super::depth`]). Falls back to
    /// `base` when the pipeline cannot be built. Call once per frame (eye 0) with the engine
    /// context mutex held.
    pub fn depth_distance(
        &mut self,
        context: &windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
        device: &jc3gi::graphics_engine::device::Device,
        cfg: &super::config::DepthShiftConfig,
        mode: super::HudMode,
        base: f32,
    ) -> f32 {
        if self.depth_shift.is_none() {
            match super::depth::DepthShift::new(device) {
                Ok(shift) => self.depth_shift = Some(shift),
                Err(e) => {
                    // Log the build failure once, not per frame; the next attempt happens on the
                    // next frame regardless.
                    static LOGGED: std::sync::atomic::AtomicBool =
                        std::sync::atomic::AtomicBool::new(false);
                    if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                        tracing::error!("hud depth: {e:#}");
                    }
                    return base;
                }
            }
        }
        let Some(shift) = self.depth_shift.as_mut() else {
            return base;
        };
        // The histogram dispatch only runs for the histogram policy; the vehicle policy needs
        // no GPU work. The mask uses the previous frame's corners (this frame's depend on the
        // distance being computed) and the redirected HUD texture's alpha.
        if cfg.use_depth_histogram
            && let Some(graphics_engine) =
                // SAFETY: the graphics engine is the live singleton on the render thread.
                unsafe { jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() }
        {
            let hud_srv = self.target.as_ref().map(|t| t.color_srv().clone());
            let mask = match (
                cfg.mask_by_hud,
                self.cached_corners,
                self.current_pose,
                &hud_srv,
            ) {
                (true, Some(corners), Some((pos, _)), Some(srv)) => {
                    Some(super::depth::MaskInputs {
                        camera_pos: pos.to_array(),
                        corners,
                        hud_srv: srv,
                    })
                }
                _ => None,
            };
            shift.sample(context, graphics_engine, cfg, mask);
        }
        shift.distance(cfg, mode, base)
    }

    /// The dynamic-distance state's live snapshot, for the debug UI.
    pub fn depth_status(&self) -> Option<super::depth::DepthShiftStatus> {
        self.depth_shift
            .as_ref()
            .map(super::depth::DepthShift::status)
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

    /// Set (or clear) the frame's marker-warp inputs. Chosen on eye 0 alongside the corners.
    pub fn set_warp_frame(&mut self, frame: Option<WarpFrame>) {
        self.warp_frame = frame;
    }

    /// Set the frame's split state (eye 0): whether the composite is live, and every layer's
    /// world-space corner set (all recomputed per frame -- the partition redraws every texture
    /// every frame, so nothing freezes).
    pub fn set_split_frame(
        &mut self,
        active: bool,
        params: Option<[super::quad::PanelParams; split::LAYER_COUNT]>,
    ) {
        self.split_composite = active;
        self.cached_layer_corners = [None, None, None];
        let Some(params) = params else {
            return;
        };
        for (slot, params) in self.cached_layer_corners.iter_mut().zip(params.iter()) {
            *slot = super::quad::compute_world_corners(params).map(|c| (c, params.distance));
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
        if self.quad.is_none() {
            return;
        }

        // While split (all layer corner sets and textures present), draw every layer at its own
        // distance, farthest first: the quads are alpha-blended overlays without a depth test, so
        // painter's order is what makes a near layer (the reticle group) occlude a far one (a
        // distant marker) and not vice versa. Otherwise, the single flat panel.
        let layer_srvs = [
            Some(hud_srv.clone()),
            self.layer_targets[0]
                .as_ref()
                .map(|t| t.color_srv().clone()),
            self.layer_targets[1]
                .as_ref()
                .map(|t| t.color_srv().clone()),
        ];
        let split_ready = self.split_composite
            && self.cached_layer_corners.iter().all(Option::is_some)
            && layer_srvs.iter().all(Option::is_some);
        if !split_ready {
            // Single-panel mode: the whole HUD texture on one surface, depth-warped per element
            // when the frame has warp inputs (markers at world depth, the reticle region at aim
            // depth), flat otherwise.
            if let Some(frame) = self.warp_frame.as_ref() {
                if self.warp.is_none() {
                    match HudWarp::new(device) {
                        Ok(warp) => self.warp = Some(warp),
                        Err(e) => tracing::error!("HUD warp: {e:#}"),
                    }
                }
                if let Some(warp) = self.warp.as_ref() {
                    let inputs = WarpInputs {
                        corners,
                        anchor: frame.anchor,
                        base_distance: frame.base_distance,
                        markers: frame.markers.clone(),
                    };
                    if warp.draw(context, device, target, &hud_srv, &inputs) {
                        return;
                    }
                }
            }
            if let Some(quad) = self.quad.as_ref() {
                quad.draw(context, device, target, &hud_srv, &corners);
            }
            return;
        }

        let distances: [f32; split::LAYER_COUNT] =
            std::array::from_fn(|i| self.cached_layer_corners[i].map_or(0.0, |(_, d)| d));
        let mut order: [usize; split::LAYER_COUNT] = [0, 1, 2];
        order.sort_by(|&a, &b| {
            distances[b]
                .partial_cmp(&distances[a])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for i in order {
            let (layer_corners, distance) = self.cached_layer_corners[i].unwrap();
            let srv = layer_srvs[i].as_ref().unwrap();
            // The marker layer draws depth-warped when the frame has warp inputs; the warp
            // pipeline is built lazily and a build failure falls back to the flat quad.
            if i == split::HudLayer::Markers as usize
                && let Some(frame) = self.warp_frame.as_ref()
            {
                if self.warp.is_none() {
                    match HudWarp::new(device) {
                        Ok(warp) => self.warp = Some(warp),
                        Err(e) => tracing::error!("HUD warp: {e:#}"),
                    }
                }
                if let Some(warp) = self.warp.as_ref() {
                    let inputs = WarpInputs {
                        corners: layer_corners,
                        anchor: frame.anchor,
                        base_distance: distance,
                        markers: frame.markers.clone(),
                    };
                    if warp.draw(context, device, target, srv, &inputs) {
                        continue;
                    }
                }
            }
            if let Some(quad) = self.quad.as_ref() {
                quad.draw(context, device, target, srv, &layer_corners);
            }
        }
    }

    /// Clear the HUD render target and depth-stencil so the next frame starts clean. A no-op when
    /// not redirected, and while the split composite is live -- the layer textures (including the
    /// main target, which doubles as the static layer) must persist between their refreshes, so
    /// the UI render detour clears each one right before re-rendering it instead. The caller must
    /// hold the engine context mutex.
    pub fn clear(&mut self, context: &ID3D11DeviceContext) {
        if !self.redirected || self.split_composite {
            return;
        }
        let Some(target) = self.target.as_ref() else {
            return;
        };
        // SAFETY: `context` is the live engine context; the RTV and DSV belong to our target
        // texture.
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

    /// Register (once) and return the egui texture ids for the marker/center layer previews.
    /// `None` per slot until the layer target exists.
    pub fn layer_preview_ids(
        &mut self,
        renderer: &mut egui_directx11::Renderer,
    ) -> [Option<egui::TextureId>; split::LAYER_COUNT - 1] {
        for (id, target) in self
            .layer_preview_ids
            .iter_mut()
            .zip(self.layer_targets.iter())
        {
            if id.is_none()
                && let Some(target) = target.as_ref()
            {
                *id = Some(renderer.register_user_texture(target.color_srv().clone()));
            }
        }
        self.layer_preview_ids
    }

    /// Drop the egui preview registration. Call on the UI thread (it owns the renderer), so the texture
    /// id is released rather than leaked.
    pub fn release_preview(&mut self, renderer: &mut egui_directx11::Renderer) {
        if let Some(id) = self.preview_id.take() {
            renderer.unregister_user_texture(id);
        }
        for id in &mut self.layer_preview_ids {
            if let Some(id) = id.take() {
                renderer.unregister_user_texture(id);
            }
        }
    }
}
