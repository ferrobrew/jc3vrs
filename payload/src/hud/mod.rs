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
//! The panel has two modes (see [`HudMode`], chosen per frame by [`current_mode`]): the gameplay HUD
//! lazily follows the head's orientation with critically-damped quaternion slerp, while full-screen
//! UI (movies, loading screens, menus) is world-static -- latched in place so the head can look away
//! from it. Each mode also has its own aspect ([`HudConfig::hud_aspect`] / [`HudConfig::movie_aspect`]).
//! The panel's world-space corners are computed once per frame (eye 0) and projected through each
//! eye's own per-eye VP, so it sits at a finite world position with correct stereo depth rather than
//! being head-locked at infinity.
//!
//! The module is split into the GPU resources ([`target`]), the game-side UI rebind operations
//! ([`binding`]), the quad draw pass ([`quad`]), the [`state`] machine that drives them, and the
//! [`config`] types for tuning parameters.

pub mod aim;
mod binding;
mod config;
pub mod cursor;
pub mod depth;
pub(crate) mod egui_panel;
pub mod markers;
pub(crate) mod pointer;
mod quad;
pub mod scaleform;
pub mod split;
mod state;
mod target;
mod warp;

pub use config::{HudConfig, ReticleAlign};
pub use state::HUD_STATE;

use glam::{Mat3, Mat4, Quat, Vec3};
use jc3gi::{
    camera::camera_manager::CameraManager,
    graphics_engine::{device::Device, texture::Texture},
    types::math::Matrix4,
    ui::ui_manager::GetIUIManager,
};
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;

/// Which presentation the HUD is in this frame. Drives the panel's aspect (gameplay vs full-screen
/// UI). Placement (head-following vs world-locked) is chosen separately by
/// [`menu_world_lock`], which only freezes for in-game menus with a valid camera.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HudMode {
    /// Gameplay HUD: rendered at [`HudConfig::hud_aspect`], head-following.
    Hud,
    /// Full-screen UI -- movies, loading screens, and menus: rendered at
    /// [`HudConfig::movie_aspect`].
    Movie,
}

/// Determine the current [`HudMode`] from the game state and UI. Any state other than in-game play
/// ([`crate::hooks::in_gameplay`] is false -- so loading screens, the frontend, and install/startup) is
/// full-screen, as is an in-game modal menu (the pause / mission / reward screens, detected via the
/// UI's static background grab).
pub fn current_mode() -> HudMode {
    if !crate::hooks::in_gameplay() {
        return HudMode::Movie;
    }
    // SAFETY: GetIUIManager returns the live UI singleton (or null before it exists);
    // IsUsingStaticBackGround is a const getter.
    let static_background = unsafe {
        GetIUIManager()
            .as_ref()
            .is_some_and(|ui| ui.IsUsingStaticBackGround())
    };
    if static_background {
        return HudMode::Movie;
    }
    HudMode::Hud
}

/// Whether the floating panel should world-lock this frame: an in-game modal menu (pause /
/// full-screen map), where the render camera still has a valid pose. Narrower than [`current_mode`]'s
/// Movie result -- it excludes the frontend and loading screens (not `E_GAME_RUN`), whose degenerate
/// camera would strand the latched pose. In-game the issue-#7 fix keeps `m_DrawScene` on, so the
/// camera pose is real and the frozen world stays visible behind the panel.
pub fn menu_world_lock() -> bool {
    // SAFETY: GetIUIManager returns the live UI singleton (or null); IsUsingStaticBackGround is a
    // const getter.
    crate::hooks::in_gameplay()
        && unsafe {
            GetIUIManager()
                .as_ref()
                .is_some_and(|ui| ui.IsUsingStaticBackGround())
        }
}

/// The effective HUD aspect (width / height) for `mode`: [`HudConfig::hud_aspect`] in gameplay,
/// [`HudConfig::movie_aspect`] for full-screen UI.
fn aspect_for(cfg: &HudConfig, mode: HudMode) -> f32 {
    match mode {
        HudMode::Hud => cfg.hud_aspect,
        HudMode::Movie => cfg.movie_aspect,
    }
}

/// The effective HUD aspect for the current frame. Used by the marker hook and panel VP, which run
/// outside [`tick`]/[`draw_quad`] and so re-derive the mode.
pub fn current_aspect() -> f32 {
    let cfg = crate::config::Config::lock_query(|c| c.hud);
    aspect_for(&cfg, current_mode())
}

/// The panel's reference width as a fraction of its distance at scale 1.0: 4 m wide at 3 m, the
/// comfortable baseline from issue #14.
const PANEL_WIDTH_PER_DISTANCE: f32 = 4.0 / 3.0;

/// The panel's physical height in meters for the given size knob, distance, and effective aspect.
///
/// The panel width is `scale * PANEL_WIDTH_PER_DISTANCE * distance`, so it grows with distance to
/// keep a constant apparent (angular) size -- moving the panel nearer or farther does not change how
/// big it looks. The height is the width divided by the aspect, so the width (and thus the amount of
/// content that fits horizontally) is invariant to aspect changes: a 16:9 panel is the same width as
/// a 1:1 panel, just shorter.
pub fn panel_height(scale: f32, distance: f32, aspect: f32) -> f32 {
    (scale * PANEL_WIDTH_PER_DISTANCE * distance) / aspect.max(f32::EPSILON)
}

/// The per-frame render-thread step: redirects the HUD into our texture while enabled, restores the
/// engine binding while disabled. Called from the render-thread post-draw hook.
///
/// `back_buffer_width`/`back_buffer_height` are the game window's back-buffer dimensions, used both
/// to size the HUD texture (via [`hud_target_size`], which scales the longer axis and applies the
/// configured aspect) and to restore the engine binding on a toggle-off. The HUD texture's aspect is
/// independent of the per-eye render aspect.
pub fn tick(device: &Device, back_buffer_width: u32, back_buffer_height: u32) {
    let mut hud = HUD_STATE.lock();
    let cfg = crate::config::Config::lock_query(|c| c.hud);
    if cfg.redirect {
        let aspect = aspect_for(&cfg, current_mode());
        let (width, height) = hud_target_size(
            cfg.render_scale,
            aspect,
            back_buffer_width,
            back_buffer_height,
        );
        hud.ensure_redirected(device, width, height, back_buffer_width, back_buffer_height);
        hud.ensure_layers(device, cfg.split);
    } else {
        hud.restore(back_buffer_width, back_buffer_height);
        hud.ensure_layers(device, false);
    }
    // Publish the frame's mouse-mapping geometry for the cursor injection (the `SendMouseEvents`
    // detour): window-client pixels normalize against the window, and rescale to the movie
    // rectangle -- our texture -- once the redirect is applied.
    cursor::set_geometry(
        (back_buffer_width, back_buffer_height),
        hud.redirected_size(),
    );
}

/// The layer views the `CUIManager::Render` detour needs for a partitioned frame, or `None` when
/// the split must not run this frame (disabled, not redirected, layer targets missing,
/// full-screen UI, or the partition not live yet). Snapshotted under the state lock so the
/// detour never holds it across the render.
pub fn split_inputs() -> Option<split::LayerViews> {
    let cfg = crate::config::Config::lock_query(|c| c.hud);
    if !cfg.redirect || !cfg.split || current_mode() != HudMode::Hud || !split::roots::live() {
        return None;
    }
    HUD_STATE.lock().split_views()
}

/// Whether the redirect and every layer target are in place, so the game-thread capture mask has
/// textures to land in. Gates the masking itself: masking without the redirected layer textures
/// would strip the visible HUD down to one layer's clips.
pub fn split_layers_ready() -> bool {
    HUD_STATE.lock().split_views().is_some()
}

/// Compute the HUD texture dimensions from the render scale, the configured aspect (width / height),
/// and the back buffer's largest axis. The longer axis is `render_scale * largest_back_buffer_axis`;
/// the shorter follows from the aspect. Both axes are clamped to at least 1 pixel so a zero-sized
/// back buffer or a degenerate aspect never reaches texture creation.
fn hud_target_size(
    render_scale: f32,
    aspect: f32,
    back_buffer_width: u32,
    back_buffer_height: u32,
) -> (u32, u32) {
    let base = render_scale * back_buffer_width.max(back_buffer_height) as f32;
    let aspect = aspect.max(f32::EPSILON);
    let (width, height) = if aspect >= 1.0 {
        (base, base / aspect)
    } else {
        (base * aspect, base)
    };
    (
        width.round().max(1.0) as u32,
        height.round().max(1.0) as u32,
    )
}

/// Draw the redirected HUD as a floating quad for `eye` over `target` (the eye's linear back buffer),
/// when both the redirect and the quad are enabled. On eye 0, chooses the panel pose for the current
/// mode (head-following or world-static) and computes the panel's world-space corners (cached for eye
/// 1). Then draws and clears. Called from the render-thread post-draw hook, before the back buffer is
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
    if eye == 0
        && let Some((head_pos, head_rotation)) = render_camera_pose()
    {
        let mode = current_mode();
        let aspect = aspect_for(&cfg, mode);
        // World-lock the panel while an in-game menu (pause / map) is open, or during a non-gameplay
        // transition in VR (loading screens / fast-travel, issue #27), so it stays put and the player
        // can look around it, reverting to head-follow on close/resume. During a loading transition the
        // panel latches to the head-tracked render pose (`render_camera_pose`), so it appears centered on
        // wherever the head was pointing as the load began and then holds that world spot while the head
        // looks around it.
        let freeze = crate::hooks::camera::vr_loading_view_active()
            || (cfg.world_lock_menus && menu_world_lock());
        let (pos, rot) = hud.update_pose(freeze, head_pos, head_rotation, &cfg.follow);
        // Dynamic panel distance: histogram the frame's depth distribution and ease the panel
        // toward the near field when the scene is close (see `depth`). The base (far) distance
        // is the manual slider; full-screen UI always reads far.
        let panel_distance = if cfg.depth_shift.enabled {
            hud.depth_distance(context, device, &cfg.depth_shift, mode, cfg.distance)
        } else {
            cfg.distance
        };
        let params_at = |distance: f32| quad::PanelParams {
            pos,
            rot,
            aspect,
            distance,
            panel_height: panel_height(cfg.panel_scale, distance, aspect),
        };
        hud.compute_world_corners(&params_at(panel_distance));
        // The virtual cursor rides the panel at its UV position, lifted toward the camera; its
        // world-space corners are computed here (eye 0) like the panel's so both eyes project the
        // same dot with correct stereo disparity (see `cursor`).
        let cursor_corners = cfg
            .cursor
            .enabled
            .then(cursor::frame)
            .flatten()
            .and_then(|frame| {
                quad::compute_cursor_corners(&params_at(panel_distance), frame, &cfg.cursor)
            });
        hud.set_cursor_corners(cursor_corners);
        // The split composites in gameplay while the render-root partition is live (the layer
        // textures then contain per-layer content, redrawn every frame).
        let split_active = cfg.split && mode == HudMode::Hud && split::roots::live();
        // The reticle depth (the split's center layer, or the single panel's center bubble)
        // follows the smoothed aim depth when enabled, easing back to a flat rest distance while
        // nothing is targeted: the center-layer distance under the split, the panel distance
        // otherwise (so a stale bubble flattens into the panel instead of poking out of it).
        let center_rest = if split_active {
            cfg.center_distance
        } else {
            panel_distance
        };
        let center_distance = if cfg.center_depth_from_aim && mode == HudMode::Hud {
            aim::current(center_rest)
        } else {
            center_rest
        };
        // The frame's recorded marker depths for the warp (recorded on the game thread by the
        // Get2DInfo hook); taken whether or not the warp draws, so stale markers never linger.
        let mut frame_markers = markers::take_frame();
        let warp_active = cfg.marker_warp && mode == HudMode::Hud;
        if split_active {
            // Every layer redraws every frame, so every corner set (and the marker warp) is
            // fresh per frame; only the stereo depth differs between layers.
            hud.set_split_frame(
                true,
                Some([
                    params_at(cfg.distance),
                    params_at(cfg.marker_distance),
                    params_at(center_distance),
                ]),
            );
            hud.set_warp_frame(warp_active.then(|| state::WarpFrame {
                anchor: pos,
                markers: frame_markers.clone(),
                base_distance: cfg.marker_distance,
            }));
        } else {
            hud.set_split_frame(false, None);
            // Single-panel mode: the reticle region joins the depth field as a center bubble at
            // the aim depth (under the split, the center layer carries the aim depth instead).
            if warp_active && cfg.center_depth_from_aim {
                frame_markers.insert(
                    0,
                    markers::MarkerDepth {
                        u: 0.5,
                        v: 0.5,
                        depth: center_distance,
                        radius: cfg.center_bubble_radius,
                    },
                );
            }
            hud.set_warp_frame(warp_active.then_some(state::WarpFrame {
                anchor: pos,
                markers: frame_markers,
                base_distance: panel_distance,
            }));
        }
    }

    hud.draw_quad(context, device, target, eye);
    hud.clear(context);
}

/// Extract the head's world-space pose `(position, rotation)` from the render camera's world
/// transform, or `None` if the camera is not available.
///
/// The camera's world transform stores its basis vectors in rows (pyxis docs): right (+X), up (+Y),
/// the +Z basis (back), then the translation. A row-major, row-vector matrix's flat array is the
/// column-major array of the equivalent glam matrix, so those rows arrive as `Mat4`'s `x_axis`,
/// `y_axis`, `z_axis`, and `w_axis`. The rotation converts to a quaternion that maps camera-local to
/// world space, so `quat * Vec3::NEG_Z` yields the forward direction.
pub(crate) fn render_camera_pose() -> Option<(Vec3, Quat)> {
    // Prefer the center (un-offset) transform snapshotted in `SetupRenderCamera` before the per-eye
    // parallax offset is applied. By the time `draw_quad` runs on eye 0, `m_TransformF` already
    // carries eye 0's half-IPD offset, which would leak into the cached panel position and double
    // the stereo disparity for eye 1. Fall back to the live transform when the snapshot is absent
    // (e.g. stereo not active, or the hook has not run yet).
    let transform = crate::stereo::STEREO_STATE
        .lock()
        .center_transform
        .or_else(|| unsafe {
            let cm = CameraManager::get()?;
            let cam = cm.m_RenderCamera.as_ref()?;
            Some(cam.m_TransformF)
        })?;

    let transform = Mat4::from(transform);
    Some((
        transform.w_axis.truncate(),
        Quat::from_mat3(&Mat3::from_mat4(transform)),
    ))
}

/// Compute the view-projection and camera matrices for the floating panel's orientation, so that
/// W2S (`Get2DInfo`) projects world points onto the panel's surface rather than the screen plane.
/// Returns `(vp, camera_transform)` in engine format, or `None` if the camera is unavailable.
///
/// The panel VP uses the damped follow rotation for orientation, the head position as the view
/// origin, and a symmetric projection built from the panel's own angular subtense (not the gameplay
/// camera's FOV). This ensures markers are positioned correctly on the floating quad: a POI directly
/// ahead of the camera but off-center from the panel's facing direction appears at the correct
/// position on the panel surface, compensating for the follow-damping lag.
pub fn compute_panel_vp(symmetric: bool) -> Option<(Matrix4, Matrix4)> {
    // Reuse the pose chosen for the quad this frame (head-following or latched world-static), so
    // markers project onto exactly the panel the quad drew.
    let (pos, rot) = HUD_STATE.lock().panel_pose()?;
    let aspect = current_aspect();
    let panel_scale = crate::config::Config::lock_query(|c| c.hud.panel_scale);

    let projection = unsafe {
        let cm = CameraManager::get()?;
        let active = cm.m_ActiveCamera.as_ref()?;
        active.m_ProjectionF
    };

    // Build the panel world transform from the pose rotation's basis vectors + the pose position.
    // The engine stores +Z as the third basis (back), so back = quat * Z (not -Z).
    let right = rot * Vec3::X;
    let up = rot * Vec3::Y;
    let back = rot * Vec3::Z;

    let panel_transform = Mat4::from_cols(
        right.extend(0.0),
        up.extend(0.0),
        back.extend(0.0),
        pos.extend(1.0),
    );

    // View = inverse(world transform). VP = P * V (glam column-vector convention).
    // The Matrix4 ↔ Mat4 From impls transpose, so the engine-format result is correct.
    let panel_view = panel_transform.inverse();
    let mut glam_proj = Mat4::from(projection);

    // The projection's field of view must match the panel's own angular subtense as seen from the
    // head -- a horizontal half-angle of `atan(scale * PANEL_WIDTH_PER_DISTANCE / 2)`, independent of
    // distance -- so that a world direction lands where the ray from the head crosses the panel
    // surface. Reusing the gameplay camera's FOV instead misplaces markers by the ratio of the game
    // FOV to the panel subtense: that ratio is ~1 on the flat desktop (the panel effectively fills
    // the screen, so its subtense ≈ the game FOV) but far from 1 in VR, where the panel is a fixed
    // floating surface viewed through a much wider per-eye FOV -- and it swings with every game FOV
    // change (aiming, sniper zoom, vehicles), sliding markers off their world features. Overwrite the
    // FOV terms with the panel's; the panel is centered on the panel-view forward, so the projection
    // is symmetric and any off-center shear is dropped. The depth (`z`) and `w` columns are kept from
    // the game projection, preserving its reverse-Z convention and the behind-camera `w`-sign cull in
    // `Convert3DCoords`. The marker pass also sets `m_CachedViewportRatio = 1 / aspect` so
    // `Convert3DCoords` does not re-apply the device aspect on top of this (see `hooks::ui`).
    // With `symmetric` false (a reticle A/B option) the override is skipped and the game camera's own
    // projection is kept, so the reticle follows the game's native screen-space aim mapping rather than
    // the panel subtense -- an alternative for tuning the crosshair-vs-shot alignment. Markers always
    // pass `symmetric` true (they misplace under the game FOV, per the note above).
    if symmetric {
        let cot_x = 2.0 / (panel_scale.max(f32::EPSILON) * PANEL_WIDTH_PER_DISTANCE);
        glam_proj.x_axis.x = cot_x;
        glam_proj.y_axis.y = cot_x * aspect.max(0.0);
        glam_proj.z_axis.x = 0.0;
        glam_proj.z_axis.y = 0.0;
    }

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
        // The clip handles must be released on the capture (game) thread; the shutdown path lets
        // a few more frames tick before the hooks come down, which drains this request.
        scaleform::request_release_handles();
    });
}
