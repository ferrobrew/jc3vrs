//! UI manager detours: world-to-screen compensation for the floating HUD panel.
//!
//! [`get_2d_info`](UIManager::Get2DInfo) is the gameplay marker placement function. When the HUD
//! is redirected and drawn as a floating quad, the VP and camera matrix are replaced with the
//! panel's orientation so that markers project onto the panel's surface rather than the screen
//! plane. See `docs/hud.md`.

use detours_macro::detour;
use jc3gi::{
    graphics_engine::graphics_engine::HContext_t,
    types::math::{Matrix4, Vector2, Vector3},
    ui::{
        scaleform::MovieImpl,
        ui_manager::{ScreenPos, UIManager},
    },
};
use re_utilities::hook_library::HookLibrary;

use crate::config::Config;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&GET_2D_INFO_BINDER)
        .with_static_binder(&UI_RENDER_BINDER)
        .with_static_binder(&CONVERT_3D_COORDS_DEFAULT_BINDER)
        .with_static_binder(&MOVIE_CAPTURE_BINDER)
}

/// Marks the start of a game frame for the aim-depth recording: `UpdateGrappleReticle`'s *first*
/// default-VP projection each frame is the game's smoothed aim position; its later calls (the
/// wire-attachment point, the grip-radius sample) are different points and must not be recorded.
/// Called from the game-thread tick.
pub fn begin_frame_aim_recording() {
    AIM_RECORDED_THIS_FRAME.store(false, std::sync::atomic::Ordering::Relaxed);
}

/// Whether the current game frame already recorded its aim depth.
static AIM_RECORDED_THIS_FRAME: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// The grapple reticle's world-to-screen (the default-VP wrapper whose only callers are in
/// `CHUDUI::UpdateGrappleReticle`). Two jobs: reproject the reticle onto the floating panel (the
/// wrapper's internal VP is not a parameter, so the reticle bypasses the `Get2DInfo` hook's panel
/// reprojection), and record the aim point's depth -- from the first call of the frame only, the
/// game's own smoothed aim position -- for the center layer's aim-driven distance.
#[detour(address = UIManager::Convert3DCoordsDefault_ADDRESS)]
fn convert_3d_coords_default(
    this: *mut UIManager,
    world: *const Vector3,
    out_x: *mut f32,
    out_y: *mut f32,
) -> bool {
    let (panel_enabled, record_aim, max_depth) = Config::lock_query(|c| {
        (
            c.hud.redirect && c.hud.quad,
            c.hud.center_depth_from_aim,
            c.hud.marker_max_depth,
        )
    });

    // SAFETY: `world` is the caller's live aim point; panel_pose only reads cached state.
    if record_aim
        && let Some(world) = unsafe { world.as_ref() }
        && let Some((anchor, _)) = crate::hud::HUD_STATE.lock().panel_pose()
    {
        let world = glam::Vec3::new(world.data[0], world.data[1], world.data[2]);
        crate::hud::aim::record((world - anchor).length().clamp(0.5, max_depth.max(0.5)));
    }

    let aspect = crate::hud::current_aspect();
    let panel = panel_enabled.then(crate::hud::compute_panel_vp).flatten();
    match panel {
        // Project through the panel VP with the aspect retarget, exactly like the Get2DInfo hook,
        // so the grapple reticle lands at the correct spot on the panel surface.
        Some((vp, _camera)) => unsafe {
            let Some(manager) = this.as_mut() else {
                return false;
            };
            let previous = manager.m_CachedViewportRatio;
            manager.m_CachedViewportRatio = 1.0 / aspect.max(f32::EPSILON);
            let ok = manager.Convert3DCoords(world, out_x, out_y, &vp);
            manager.m_CachedViewportRatio = previous;
            ok
        },
        None => CONVERT_3D_COORDS_DEFAULT
            .get()
            .unwrap()
            .call(this, world, out_x, out_y),
    }
}

/// The capture seam: `CUIManager::PreRender` calls `MovieImpl::Capture` right after `Advance`,
/// on the game update thread with the deferred render lock held -- the only point in the frame
/// where every display-tree writer is quiescent. The render-root partition maintains itself here
/// (build, per-frame reconcile against pool churn, teardown), and the issue #8 overlay
/// suppression writes here, so the engine's own capture carries everything; the original then
/// runs unchanged. Movies other than the main UI movie (the render-to-texture instances) pass
/// through untouched. See [`crate::hud::roots`] and [`crate::hud::split`].
#[detour(address = MovieImpl::CaptureImpl_ADDRESS)]
fn movie_capture(this: *mut MovieImpl, if_changed: bool) -> u64 {
    // SAFETY: the UI manager is a live singleton past startup; the pointer comparison does not
    // dereference `this`.
    let is_main_movie =
        unsafe { UIManager::get().is_some_and(|manager| std::ptr::eq(manager.m_Movie, this)) };
    if is_main_movie {
        let (split_enabled, suppress_overlays) = Config::lock_query(|c| {
            (
                c.hud.redirect && c.hud.quad && c.hud.split,
                c.hud.suppress_overlays,
            )
        });
        let split_active = split_enabled
            && crate::hud::current_mode() == crate::hud::HudMode::Hud
            && crate::hud::split_layers_ready();
        // SAFETY: this is the capture seam both callees require; `this` is the live main movie.
        unsafe {
            crate::hud::split::apply_overlay_suppression(suppress_overlays);
            if let Some(movie) = this.as_mut() {
                crate::hud::roots::on_capture(movie, split_active);
            }
        }
    }
    MOVIE_CAPTURE.get().unwrap().call(this, if_changed)
}

/// Render the partitioned HUD (each render root into its own layer texture, full rate) while the
/// partition is live; otherwise pass through. Runs on the UI render worker (kicked by
/// `StartRender`). See [`crate::hud::roots`].
#[detour(address = UIManager::Render_ADDRESS)]
fn ui_render(this: *mut UIManager, context: *mut HContext_t) {
    let original = UI_RENDER.get().unwrap();
    if let Some(views) = crate::hud::split_inputs() {
        // SAFETY: called from the detour with the detour's own arguments, on the UI render
        // worker.
        if unsafe { crate::hud::roots::render_partitioned(this, &views) } {
            return;
        }
    }
    original.call(this, context);
}

/// Replace the VP and camera matrix in `Get2DInfo` with the floating panel's orientation, so
/// world-to-screen projects markers onto the panel surface rather than the screen plane. When the
/// HUD redirect or quad is off, the original caller's VP is passed through unchanged.
//
// The parameter count matches the game's `CUIManager::Get2DInfo` ABI — the detour macro requires an
// exact signature match, so the parameters cannot be bundled into a struct.
#[detour(address = UIManager::Get2DInfo_ADDRESS)]
#[allow(clippy::too_many_arguments)]
fn get_2d_info(
    this: *mut UIManager,
    world: *const Vector3,
    vp_orig: *const Matrix4,
    camera_orig: *const Matrix4,
    a5: f32,
    out_x: *mut f32,
    out_y: *mut f32,
    out_pos: *mut ScreenPos,
    margin: f32,
    a10: bool,
    offset: Vector2,
) {
    let (panel_enabled, record_depths, max_depth, marker_radius) = Config::lock_query(|c| {
        (
            c.hud.redirect && c.hud.quad,
            c.hud.redirect && c.hud.quad && c.hud.marker_warp,
            c.hud.marker_max_depth,
            c.hud.marker_radius,
        )
    });
    let aspect = crate::hud::current_aspect();
    let panel = panel_enabled.then(crate::hud::compute_panel_vp).flatten();
    let (vp, camera) = panel
        .as_ref()
        .map(|(v, c)| (v as *const Matrix4, c as *const Matrix4))
        .unwrap_or((vp_orig, camera_orig));

    // For the panel pass, retarget `Convert3DCoords`' aspect correction to the panel: it reads
    // `m_CachedViewportRatio` (the window height/width, refreshed every frame), which would skew
    // markers off the panel on top of the already-re-aspected panel VP. Set it to the panel's
    // `height / width` (= `1 / aspect`) for the call and restore it afterwards so other
    // world-to-screen consumers in the same frame are unaffected.
    let restore_ratio = panel
        .is_some()
        .then(|| unsafe {
            this.as_mut().map(|manager| {
                let previous = manager.m_CachedViewportRatio;
                manager.m_CachedViewportRatio = 1.0 / aspect.max(f32::EPSILON);
                previous
            })
        })
        .flatten();

    GET_2D_INFO.get().unwrap().call(
        this, world, vp, camera, a5, out_x, out_y, out_pos, margin, a10, offset,
    );

    if let Some(previous) = restore_ratio {
        unsafe {
            if let Some(manager) = this.as_mut() {
                manager.m_CachedViewportRatio = previous;
            }
        }
    }

    // Record on-screen markers for the marker-layer depth warp: where the marker landed on the
    // panel texture (stage coordinates over the cached stage size) and how far its world point is
    // from the panel anchor. Edge-clamped markers are directional indicators, not world points,
    // and stay at the layer's base depth.
    // SAFETY: the original call populated the out pointers; `this` and `world` are the caller's
    // live arguments.
    if record_depths && panel.is_some() {
        unsafe {
            if let (Some(manager), Some(world), Some(&pos)) =
                (this.as_ref(), world.as_ref(), out_pos.as_ref())
                && pos == ScreenPos::SCREEN_POS_ONSCREEN
                && manager.m_CachedStageWidth > 0.0
                && manager.m_CachedStageHeight > 0.0
                && let Some((anchor, _)) = crate::hud::HUD_STATE.lock().panel_pose()
            {
                let world = glam::Vec3::new(world.data[0], world.data[1], world.data[2]);
                let depth = (world - anchor).length().clamp(0.5, max_depth.max(0.5));
                crate::hud::markers::record(crate::hud::markers::MarkerDepth {
                    u: *out_x / manager.m_CachedStageWidth,
                    v: *out_y / manager.m_CachedStageHeight,
                    depth,
                    radius: marker_radius,
                });
            }
        }
    }
}
