//! The interactive egui debug panel, floating as a 2D surface in 3D space in VR (issue #24).
//!
//! In VR the flat egui overlay that rides the game back buffer is invisible -- the desktop cursor
//! lands in neither eye and the overlay is suppressed by `BLOCK_FLIP` -- so the debug UI is redirected
//! into an offscreen texture and drawn back into the scene as a head-following floating quad, exactly
//! like the gameplay HUD panel (see [`super::quad`]), and driven by the desktop mouse re-sourced onto
//! the panel surface (see [`super::pointer`]).
//!
//! This is a wholly independent panel: its own render target, its own lazy-follow damping, and its own
//! eye-0-cached corners, so it never touches [`super::state::HUD_STATE`] and cannot perturb the
//! gameplay HUD. The whole feature is gated behind [`is_active`] -- an OpenXR session running *and* the
//! opt-in [`EguiPanelConfig::enabled`](crate::hud::config) flag -- and defaults off, so with it off
//! every path is byte-identical to the flat overlay baseline.
//!
//! egui is tessellated exactly once per frame (the single-use `RendererOutput`), on eye 0, into the
//! panel texture; both eyes then draw the same cached world-space corners through their own per-eye VP
//! (the same compute-on-eye-0 rule as the gameplay HUD), and the desktop mirror composites the same
//! panel texture (see [`crate::vr::mirror`]). There is never a second egui pass.

use std::time::Instant;

use glam::Quat;
use jc3gi::graphics_engine::{device::Device, texture::Texture};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::{ID3D11DeviceContext, ID3D11ShaderResourceView};

use super::{
    config::{EguiPanelConfig, FollowConfig},
    cursor::CursorFrame,
    quad::{HudQuad, PanelParams, compute_cursor_corners, compute_world_corners},
    target::HudTarget,
};

/// The panel pipeline and per-frame cache. Locked briefly on the render thread (and, for the SRV
/// handoff, on the game thread from the mirror).
static EGUI_PANEL: Mutex<EguiPanelState> = Mutex::new(EguiPanelState::new());

/// Whether the egui floating panel is active this frame: the opt-in panel is enabled and a VR frame
/// is rendering. The single predicate gating every panel path; off is the untouched baseline.
///
/// This runs on the render thread (per-eye `draw_quad`), which must never touch the `VR_STATE` runtime
/// lock: the frame holds that lock across the eye draws (`FrameContext`), so locking it here (as an
/// earlier `vr::is_running()` did) deadlocks the draw against `WaitForCPUDrawToFinish` -- even with the
/// panel disabled, since the config check came second. The enabled flag is checked first (cheap, and
/// `CONFIG` is never held across a draw), and the VR-frame signal is the draw-safe [`crate::vr::render_params`]
/// slot (a separate lock, published per frame precisely so draw-thread hooks can read it), not the
/// runtime lock.
pub(crate) fn is_active() -> bool {
    crate::config::Config::lock_query(|c| c.hud.egui_panel.enabled)
        && crate::vr::render_params(0).is_some()
}

/// The panel texture size when the panel is active, or `None` otherwise. The egui layout is sized to
/// this ([`crate::egui_impl::EguiState::set_panel_mode`]) and the pointer maps into it.
pub(crate) fn active_size() -> Option<(u32, u32)> {
    is_active().then(|| crate::config::Config::lock_query(|c| c.hud.egui_panel.resolution))
}

/// A clone of the panel texture's shader-resource view for the desktop mirror composite, or `None`
/// while the panel target does not exist yet. Locks [`EGUI_PANEL`] briefly.
pub(crate) fn panel_srv() -> Option<ID3D11ShaderResourceView> {
    EGUI_PANEL
        .lock()
        .target
        .as_ref()
        .map(|t| t.color_srv().clone())
}

/// Draw the egui panel for `eye` over `target` (the eye's linear back buffer). On eye 0 -- and only
/// while [`is_active`] -- the egui frame is rendered into the panel texture, the head-following pose
/// is chosen, and the world-space corners (panel + cursor) are computed and cached. Both eyes then
/// draw the cached corners. A no-op while inactive. Called from the render-thread post-draw hook after
/// the gameplay HUD quad, with the engine context mutex held.
pub(crate) fn draw_quad(
    context: &ID3D11DeviceContext,
    device: &Device,
    target: &Texture,
    eye: usize,
) {
    if !is_active() {
        return;
    }
    let cfg = crate::config::Config::lock_query(|c| c.hud);
    let mut panel = EGUI_PANEL.lock();
    if eye == 0 {
        panel.prepare(context, device, &cfg.egui_panel, &cfg.cursor);
    }
    panel.draw(context, device, target);
}

/// The panel's render target, quad pipeline, follow damping, and eye-0 corner cache.
struct EguiPanelState {
    target: Option<HudTarget>,
    quad: Option<HudQuad>,
    follow: PanelFollow,
    /// World-space panel corners, computed on eye 0 and reused for eye 1 (correct stereo depth).
    cached_corners: Option<[[f32; 4]; 4]>,
    /// The virtual cursor's world-space corners, computed on eye 0 alongside the panel corners.
    cursor_corners: Option<[[f32; 4]; 4]>,
}

impl EguiPanelState {
    const fn new() -> Self {
        Self {
            target: None,
            quad: None,
            follow: PanelFollow::new(),
            cached_corners: None,
            cursor_corners: None,
        }
    }

    /// Eye-0 setup: (re)create the panel target at the configured resolution, render this frame's egui
    /// output into it, choose the pose, and compute + cache the panel and cursor corners.
    fn prepare(
        &mut self,
        context: &ID3D11DeviceContext,
        device: &Device,
        cfg: &EguiPanelConfig,
        cursor_cfg: &super::config::CursorConfig,
    ) {
        self.cached_corners = None;
        self.cursor_corners = None;

        let (width, height) = (cfg.resolution.0.max(1), cfg.resolution.1.max(1));
        if self.target.as_ref().map(HudTarget::size) != Some((width, height)) {
            match HudTarget::new(device, width, height) {
                Ok(target) => self.target = Some(target),
                Err(e) => {
                    tracing::error!("egui panel target: {e:#}");
                    self.target = None;
                    return;
                }
            }
        }
        let Some(target) = self.target.as_ref() else {
            return;
        };

        // Render the single-use egui output into the panel texture. A no-op if the output was already
        // consumed this frame (e.g. the flat path ran first), which leaves the previous panel content.
        if let Some(egui_state) = crate::egui_impl::EguiState::get().as_mut() {
            egui_state.render_to(context, target.color_rtv());
        }

        let Some((head_pos, head_rotation)) = super::render_camera_pose() else {
            return;
        };
        let rot = self.follow.update(head_rotation, &cfg.follow);
        let aspect = cfg.aspect.max(f32::EPSILON);
        let params = PanelParams {
            pos: head_pos,
            rot,
            aspect,
            distance: cfg.distance,
            panel_height: super::panel_height(cfg.scale, cfg.distance, aspect),
        };
        self.cached_corners = compute_world_corners(&params);
        self.cursor_corners = super::pointer::window_uv()
            .and_then(|(u, v)| compute_cursor_corners(&params, CursorFrame { u, v }, cursor_cfg));
    }

    /// Draw the cached panel and cursor corners over `target` for the current eye. A no-op until the
    /// pipeline, target, and corners exist. The quad pipeline is built lazily on the first draw.
    fn draw(&mut self, context: &ID3D11DeviceContext, device: &Device, target: &Texture) {
        let Some(corners) = self.cached_corners else {
            return;
        };
        let Some(srv) = self.target.as_ref().map(|t| t.color_srv().clone()) else {
            return;
        };
        if self.quad.is_none() {
            match HudQuad::new(device) {
                Ok(quad) => self.quad = Some(quad),
                Err(e) => {
                    tracing::error!("egui panel quad: {e:#}");
                    return;
                }
            }
        }
        let Some(quad) = self.quad.as_ref() else {
            return;
        };
        quad.draw(context, device, target, &srv, &corners);
        if let Some(cursor_corners) = self.cursor_corners {
            quad.draw_cursor(context, device, target, &cursor_corners);
        }
    }
}

/// Critically-damped quaternion-slerp follow for the panel orientation, independent of the gameplay
/// HUD's follow so the two never share state.
struct PanelFollow {
    rotation: Quat,
    last_time: Option<Instant>,
    initialized: bool,
}

impl PanelFollow {
    const fn new() -> Self {
        Self {
            rotation: Quat::IDENTITY,
            last_time: None,
            initialized: false,
        }
    }

    /// Ease the panel rotation toward the head's current rotation: `alpha = 1 - 2^(-dt/halflife)`. The
    /// first frame snaps to the head so the panel does not swing in from identity.
    fn update(&mut self, head_rotation: Quat, config: &FollowConfig) -> Quat {
        if !self.initialized {
            self.initialized = true;
            self.rotation = head_rotation;
            self.last_time = Some(Instant::now());
            return self.rotation;
        }
        let dt = self
            .last_time
            .map(|t| t.elapsed().as_secs_f32())
            .unwrap_or(0.016)
            .min(0.1);
        self.last_time = Some(Instant::now());
        let alpha = (1.0 - 2.0_f32.powf(-dt / config.rotation_halflife.max(f32::EPSILON))).min(1.0);
        self.rotation = self.rotation.slerp(head_rotation, alpha);
        self.rotation
    }
}
