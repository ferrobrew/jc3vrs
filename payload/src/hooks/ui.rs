//! UI manager detours: world-to-screen compensation for the floating HUD panel.
//!
//! [`get_2d_info`](UIManager::Get2DInfo) is the gameplay marker placement function. When the HUD
//! is redirected and drawn as a floating quad, the VP and camera matrix are replaced with the
//! panel's orientation so that markers project onto the panel's surface rather than the screen
//! plane. See `docs/mod/hud.md`.

use detours_macro::detour;
use jc3gi::{
    graphics_engine::graphics_engine::HContext_t,
    types::math::{Matrix4, Vector2, Vector3},
    ui::{
        overlay_ui::OverlayUI,
        scaleform::{MouseEvent, MovieImpl},
        ui_manager::{ScreenPos, UIManager},
    },
};
use re_utilities::hook_library::HookLibrary;

use crate::{config::Config, hud::cursor};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&GET_2D_INFO_BINDER)
        .with_static_binder(&UI_RENDER_BINDER)
        .with_static_binder(&CONVERT_3D_COORDS_DEFAULT_BINDER)
        .with_static_binder(&MOVIE_CAPTURE_BINDER)
        .with_static_binder(&SEND_MOUSE_EVENTS_BINDER)
        .with_static_binder(&GET_MOVIE_SPACE_MOUSE_CURSOR_BINDER)
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
/// through untouched. See [`crate::hud::split::roots`] and [`crate::hud::scaleform`].
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
        // The partition persists across menus and pauses: the render detour only composites
        // per-layer in gameplay, and the main root (where menus live) renders normally either
        // way. Tearing down per mode change would churn the render tree with hundreds of
        // structural ops per pause, for no benefit.
        let split_active = split_enabled
            && crate::hud::scaleform::handles_hud_fresh()
            && crate::hud::split_layers_ready();
        // SAFETY: this is the capture seam both callees require; `this` is the live main movie.
        unsafe {
            crate::hud::scaleform::apply_overlay_suppression(suppress_overlays);
            if let Some(movie) = this.as_mut() {
                crate::hud::split::roots::on_capture(movie, split_active);
                log_pipeline_lag(movie, split_active);
            }
        }
    }
    MOVIE_CAPTURE.get().unwrap().call(this, if_changed)
}

/// Log the snapshot pipeline's produced-vs-displayed gap every few seconds: the displayed UI
/// trailing the update thread by a growing number of captures means the render side is not
/// consuming them (the "elements update late" symptom); a steady small gap acquits the snapshot
/// pipeline and points at the update side instead.
fn log_pipeline_lag(movie: &MovieImpl, split_active: bool) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static LAST_LOG: parking_lot::Mutex<Option<std::time::Instant>> = parking_lot::Mutex::new(None);
    static LAST_ACTIVE: AtomicU64 = AtomicU64::new(0);
    let due = {
        let mut last = LAST_LOG.lock();
        let due = last.is_none_or(|t| t.elapsed().as_secs_f32() >= 5.0);
        if due {
            *last = Some(std::time::Instant::now());
        }
        due
    };
    if !due {
        return;
    }
    let ids = movie.RenderContext.SnapshotFrameIds;
    let produced_in_window = ids[0].wrapping_sub(LAST_ACTIVE.swap(ids[0], Ordering::Relaxed));
    tracing::debug!(
        "scaleform pipeline: active {} displaying {} (lag {}), {} captures in window, \
         partition {}",
        ids[0],
        ids[2],
        ids[0].saturating_sub(ids[2]),
        produced_in_window,
        if split_active { "active" } else { "inactive" },
    );
}

/// Render the partitioned HUD (each render root into its own layer texture, full rate) while the
/// partition is live; otherwise pass through. Runs on the UI render worker (kicked by
/// `StartRender`). See [`crate::hud::split::roots`].
#[detour(address = UIManager::Render_ADDRESS)]
fn ui_render(this: *mut UIManager, context: *mut HContext_t) {
    let original = UI_RENDER.get().unwrap();
    if let Some(views) = crate::hud::split_inputs() {
        // SAFETY: called from the detour with the detour's own arguments, on the UI render
        // worker.
        if unsafe { crate::hud::split::roots::render_partitioned(this, &views) } {
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

/// Replace the game's mouse-to-UI coordinate mapping while the HUD redirect is active.
///
/// The original converts window-client pixels to movie-viewport pixels by subtracting the
/// centering offset `(m_CachedViewport - m_MovieScale) / 2` -- but the redirect points both of
/// those at our offscreen texture, so window coordinates no longer relate to either and the
/// original's mapping lands nowhere near the pointer. It also only emits a move event on frames
/// where the DirectInput mouse reported a delta, and reads clicks out of the steering action map.
///
/// Instead: normalize the window-client position ([`UIManager::m_MouseX`], written by `WndProc` on
/// every `WM_MOUSEMOVE`) against the window size, rescale to the movie rectangle (= our texture),
/// and hand Scaleform the whole mouse state -- position plus the `WndProc`-tracked button bitmask
/// -- via `NotifyMouseState`, which diffs against the previous state on the next `Advance` to
/// synthesize hover/press/release. The wheel still needs an explicit event. The game's own
/// `MCI_cursor` sprite is parked offscreen (it would otherwise be drawn into the panel texture
/// alongside our own cursor), and the panel cursor's position and visibility are published for the
/// render side (see [`crate::hud::cursor`]).
///
/// Like the original, it runs both from `WndProc` (via `SetMousePos`) and once per frame from
/// `CUIManager::PreUpdate`, so the state stays fresh even while the physical mouse is still.
#[detour(address = UIManager::SendMouseEvents_ADDRESS)]
fn send_mouse_events(this: *mut UIManager, steering: *mut std::ffi::c_void) -> bool {
    let original = SEND_MOUSE_EVENTS.get().unwrap();

    let active = Config::lock_query(|c| c.hud.redirect && c.hud.cursor.enabled);
    // While egui captures input the debug panel owns the mouse; leave the game's own path in
    // place (its input manager is disabled anyway) and hide the panel cursor.
    let egui_captured = crate::egui_impl::EguiState::get()
        .as_ref()
        .is_some_and(|s| s.is_input_captured());
    // The mapping geometry is published by the render-thread HUD tick only once the redirect is
    // applied; until then the original's coordinate spaces are still intact.
    let geometry = cursor::geometry();
    if !active || egui_captured || geometry.is_none() {
        cursor::set_frame(None);
        return original.call(this, steering);
    }
    let ((window_w, window_h), (movie_w, movie_h)) = geometry.unwrap();

    // SAFETY: `this` is the live UI singleton (the engine's own callers pass it); the movie
    // pointer is checked before use.
    let Some(manager) = (unsafe { this.as_mut() }) else {
        return original.call(this, steering);
    };
    let Some(movie) = (unsafe { manager.m_Movie.as_mut() }) else {
        cursor::set_frame(None);
        return false;
    };

    // Mirror the original's gamepad handling: park the Scaleform mouse out of the movie so
    // gamepad-driven menu focus is not fought by a stale hover.
    if manager.m_IsUsingGamepad {
        // SAFETY: NotifyMouseState is the movie's own embedding API, called from the same
        // contexts the engine calls HandleEvent from.
        unsafe { movie.NotifyMouseState(-1000.0, -1000.0, 0, 0) };
        cursor::set_frame(None);
        return true;
    }

    let u = (manager.m_MouseX as f32 / window_w as f32).clamp(0.0, 1.0);
    let v = (manager.m_MouseY as f32 / window_h as f32).clamp(0.0, 1.0);

    // Map through the movie's LIVE stage-to-viewport matrix rather than assuming any pixel
    // scale: HandleEvent/NotifyMouseState invert `MovieImpl.ViewportMatrix` to produce the stage
    // point they hit-test, and that copy of the matrix can be stale relative to the render
    // root's (observed in practice: the render root carries the texture-shaped matrix while the
    // movie still holds the load-time stage-identity one, so injected texture pixels were
    // consumed as stage pixels -- a uniform ~0.73 compression toward the top-left). Computing
    // the desired stage point (the UI fills the texture with the stage) and running it FORWARD
    // through the live matrix is exact no matter which matrix the movie currently holds.
    // The matrix maps stage twips (20/px) to view-rectangle pixels: `px = s * twips + t`.
    let stage_w = manager.m_CachedStageWidth;
    let stage_h = manager.m_CachedStageHeight;
    let m = movie.ViewportMatrix;
    let (x, y) = if stage_w > 0.0 && stage_h > 0.0 && m[0] != 0.0 && m[5] != 0.0 {
        let stage_x_twips = u * stage_w * 20.0;
        let stage_y_twips = v * stage_h * 20.0;
        (m[0] * stage_x_twips + m[3], m[5] * stage_y_twips + m[7])
    } else {
        // Degenerate stage or matrix (movie still initializing): fall back to texture pixels.
        (u * movie_w as f32, v * movie_h as f32)
    };

    // SAFETY: as above; the wheel event matches the layout the engine itself builds for
    // HandleEvent, and the overlay call no-ops unless the overlay is active.
    unsafe {
        movie.NotifyMouseState(x, y, cursor::buttons(), 0);

        let wheel_lines = cursor::take_wheel_lines();
        if wheel_lines != 0.0 {
            let mut event: MouseEvent = std::mem::zeroed();
            event.Type = MouseEvent::TYPE_MOUSE_WHEEL;
            event.x = x;
            event.y = y;
            event.ScrollDelta = wheel_lines;
            movie.HandleEvent(&event);
        }

        // Park the game's in-movie cursor sprite: with the original bypassed nothing repositions
        // it, and its last position would ghost inside the panel texture under our own cursor.
        if let Some(overlay) = OverlayUI::get() {
            overlay.SetMouseCursorPosition(-10_000.0, -10_000.0);
        }
    }

    // The panel cursor shows exactly when the game's own policy would show a cursor: the overlay
    // visibility refcount is driven per frame by `CUIManager::MousePointerVisibility` (overlay
    // active, no gamepad, no cursor-hiding HUD state).
    // SAFETY: reads a field of the live overlay singleton.
    let visible =
        unsafe { OverlayUI::get() }.is_some_and(|overlay| overlay.m_MouseCursorShowRefCount > 0);
    cursor::set_frame(visible.then_some(cursor::CursorFrame { u, v }));
    true
}

/// The map's mouse-to-stage conversion: `CCommMapUI::OnManageInput` -- the function's only
/// caller -- feeds `GetMousePos` window-client pixels through it to get the stage-space cursor
/// position it uses for icon picking, click selection, drag panning, and zoom-to-cursor. The
/// original maps `(pos - m_MouseDelta) * m_MouseScaleFac`, and those fields are written only by
/// `ComputeMovieSizeOnViewSize` -- which the redirect bypasses, leaving them describing the
/// window-shaped movie rectangle (X happens to stay correct because its delta is zero and the
/// scale factor cancels; Y is offset by the stale half-letterbox). Generic Scaleform widgets are
/// unaffected -- they hit-test through the injected `NotifyMouseState` path -- so the map was the
/// one UI screen left broken. Replace the mapping with the same client-rect normalization the
/// injection uses, so the map's cursor and the panel dot agree by construction.
#[detour(address = UIManager::GetMovieSpaceMouseCursor_ADDRESS)]
fn get_movie_space_mouse_cursor(
    this: *const UIManager,
    viewport_x: f32,
    viewport_y: f32,
    out: *mut jc3gi::types::math::Vector2,
) {
    let original = GET_MOVIE_SPACE_MOUSE_CURSOR.get().unwrap();
    let geometry = Config::lock_query(|c| c.hud.redirect)
        .then(cursor::geometry)
        .flatten();
    // SAFETY: `this` is the live UI singleton (the map passes it); `out` is the caller's
    // out-pointer, written exactly once.
    unsafe {
        let (Some((window, _)), Some(manager)) = (geometry, this.as_ref()) else {
            return original.call(this, viewport_x, viewport_y, out);
        };
        let (stage_w, stage_h) = (manager.m_CachedStageWidth, manager.m_CachedStageHeight);
        if stage_w <= 0.0 || stage_h <= 0.0 {
            return original.call(this, viewport_x, viewport_y, out);
        }
        let u = (viewport_x / window.0 as f32).clamp(0.0, 1.0);
        let v = (viewport_y / window.1 as f32).clamp(0.0, 1.0);
        if let Some(out) = out.as_mut() {
            out.data = [u * stage_w, v * stage_h];
        }
    }
}
