use std::{
    ffi::c_void,
    sync::atomic::{AtomicU32, Ordering},
};

use detours_macro::detour;
use jc3gi::{
    camera::{
        camera::Camera,
        camera_context::{CameraContext, CameraControlContext},
        camera_manager::CameraManager,
        game_camera_manager::GameCameraManager,
    },
    character::character::{Character, SafeBoneIndex},
    graphics_engine::graphics_engine::GraphicsEngine,
    hash::hashlittle,
    types::math::Matrix4,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    debug::trace::{TraceEvent, TraceState},
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&CAMERA_UPDATE_RENDER_BINDER)
        .with_static_binder(&CAMERA_TREE_UPDATE_RENDER_CONTEXTS_BINDER)
        .with_static_binder(&SETUP_RENDER_CAMERA_BINDER)
        .with_static_binder(&GAME_CAMERA_MANAGER_GET_CAMERA_MATRIX_BINDER)
}

/// The last `dtf` the active camera's `UpdateRender` received, for the debug UI: the engine's
/// sub-frame interpolation fraction (see `docs/issues/20-animation-judder.md`). If it sits at 0.0
/// or 1.0 every frame, the engine's T0 → T1 lerp is inert and nothing smooths the sim-tick
/// cadence.
pub fn last_dtf() -> f32 {
    f32::from_bits(LAST_DTF.load(Ordering::Relaxed))
}

/// The render camera's world transform (`m_TransformF`) and view (`m_View`) captured while
/// [`crate::config::StereoConfig::freeze_render_camera`] is on, reused every Draw so the camera holds
/// still (issue #31 isolation, Test C). `None` when the toggle is off, so re-enabling recaptures the
/// then-current pose.
static FROZEN_RENDER_CAMERA: Mutex<Option<([f32; 16], [f32; 16])>> = Mutex::new(None);

/// The scene render camera is the engine-owned copy (`GraphicsEngine::m_RenderCamera`), rebuilt
/// each Draw by `SetupRenderCamera` (reverse-Z + jitter, then `m_ViewProjection`/`m_ViewProjectionF`
/// from `m_View`). For the stereo double-Draw we offset that copy's `m_View` laterally per eye,
/// *before* the rebuild, so the two dispatches diverge. See `docs/engine/rendering.md` section 2.
#[detour(address = jc3gi::camera::camera::Camera::SetupRenderCamera_ADDRESS)]
fn setup_render_camera(camera: *mut Camera, jitter: bool) -> *mut c_void {
    let is_render_camera = unsafe {
        GraphicsEngine::get().is_some_and(|ge| std::ptr::eq(&raw const ge.m_RenderCamera, camera))
    };
    if is_render_camera {
        TraceState::record_eye(TraceEvent::SetupRenderCamera);
        let mut stereo = crate::stereo::STEREO_STATE.lock();
        // Clear the shadow-anchor delta; the parallax block below sets it when disparity is on, so a
        // stale value can't leak into the sun-shadow cascade correction when disparity is off.
        stereo.shadow_anchor_delta = [0.0; 3];
        // Eye 0 opens a new real frame: last frame's view-projection snapshots become "previous".
        if stereo.draw_index == 0 {
            stereo.vp_history.rotate();
        }
    }

    // Snapshot the stereo + FSR config once; drop the lock before the engine call below.
    let (stereo_active, force_smaa_1x, stereo_cameras, ipd, fsr_enabled) = {
        let active = crate::stereo::active();
        Config::lock_query(|c| {
            (
                active,
                c.stereo.force_smaa_1x,
                c.stereo.cameras,
                c.stereo.ipd,
                c.fsr.enabled,
            )
        })
    };

    // The engine's TAA jitter only feeds its own SMAA T2X. Drop it when that resolve is gone --
    // either because we force SMAA 1x in stereo, or because FSR has replaced the engine AA. FSR still
    // needs jitter, but its own Halton sequence, applied below.
    let jitter = jitter && !fsr_enabled && !(stereo_active && force_smaa_1x);

    // Flicker-isolation diagnostic (issue #31, Test C): pin the render camera's world transform and view
    // to a value captured when the toggle is enabled, so the game camera holds still while the sun and
    // the rest of the sim keep moving -- isolating a sun-driven per-frame flicker from a camera-idle one.
    // Applied before SetupRenderCamera so the engine rebuilds the view-projections (and fits the shadow
    // cascade) from the frozen centre; the per-eye offset below still runs on top. See
    // `StereoConfig::freeze_render_camera`.
    if is_render_camera {
        let freeze = Config::lock_query(|c| c.stereo.freeze_render_camera);
        let mut frozen = FROZEN_RENDER_CAMERA.lock();
        if freeze {
            if let Some(camera) = unsafe { camera.as_mut() } {
                let (transform, view) =
                    frozen.get_or_insert((camera.m_TransformF.data, camera.m_View.data));
                camera.m_TransformF.data = *transform;
                camera.m_View.data = *view;
            }
        } else {
            *frozen = None;
        }
    }

    // The VR per-eye off-axis projection and world offset (docs/mod/vr-runtime.md blockers 1 & 3).
    // Fetch this eye's parameters once; `None` on flatscreen frames. Under the preferred convention,
    // write the standard-depth off-axis projection into `m_Projection` *before* SetupRenderCamera so
    // the engine applies its reverse-Z remap and TAA jitter to it exactly once (§2.7).
    let vr_eye = is_render_camera
        .then(|| crate::vr::render_params(crate::stereo::draw_index()))
        .flatten();
    if let Some(vr) = vr_eye
        && vr.convention == crate::vr::ProjectionConvention::EnginePreReverseZ
        && let Some(camera) = unsafe { camera.as_mut() }
    {
        camera.m_Projection.data = vr.projection_standard;
    }

    let result = SETUP_RENDER_CAMERA.get().unwrap().call(camera, jitter);

    // Snapshot the center view-projection before the FSR-jitter and per-eye blocks below patch it.
    // This is the value the engine's own sim-side previous-VP snapshot holds (un-offset, unjittered
    // -- the engine jitter is disabled above whenever we patch), which the velocity pass reprojects
    // with; the FSR motion-vector correction needs it as its "what the engine encoded" matrix.
    if is_render_camera && let Some(camera) = unsafe { camera.as_ref() } {
        // Coordinate-frame verification (docs/mod/vr-runtime.md "Blocker 3"): log the pristine center
        // camera's basis + travel direction before the per-eye offset below mutates m_TransformF.
        crate::debug::coord_frame::log_render_camera_frame(camera);

        crate::stereo::STEREO_STATE.lock().vp_history.cur_center =
            Some(glam::Mat4::from(camera.m_ViewProjectionF));
    }

    // FSR is a temporal reconstructor: it needs the camera jittered by its sequence, with the same
    // offset fed to the dispatch. Apply it to the render camera's projections after SetupRenderCamera
    // has built them (reverse-Z done), then rebuild the view-projections from the jittered proj.
    if is_render_camera && fsr_enabled {
        unsafe {
            if let Some(camera) = camera.as_mut()
                && let Some(ge) = GraphicsEngine::get()
                && let Some(mc) = ge.m_MainColorBuffer.as_ref()
            {
                let (w, h) = (u32::from(mc.m_Width), u32::from(mc.m_Height));
                crate::fsr::apply_jitter_to_projection(&mut camera.m_Projection, w, h);
                crate::fsr::apply_jitter_to_projection(&mut camera.m_ProjectionF, w, h);
                // Publish the UV-space shift this jitter applies to every projected position, for
                // the motion-vector jitter cancellation (the velocity pass measures curUV under the
                // jittered projection, so every vector carries this shift as a constant offset).
                let jitter_uv = crate::fsr::current_camera_jitter_ndc(w, h)
                    .map_or((0.0, 0.0), |(x, y)| (0.5 * x, -0.5 * y));
                crate::stereo::STEREO_STATE.lock().vp_history.cur_jitter_uv = jitter_uv;
                let view = &camera.m_View as *const Matrix4;
                let proj = &camera.m_Projection as *const Matrix4;
                let proj_f = &camera.m_ProjectionF as *const Matrix4;
                Matrix4::Multiply4x4(view, proj, &mut camera.m_ViewProjection);
                Matrix4::Multiply4x4(view, proj_f, &mut camera.m_ViewProjectionF);
            }
        }
    }

    // Per-eye parallax: shift the camera world position (m_TransformF translation == camera+0x84,
    // the CameraPosition the camera-relative scene render subtracts) per eye. In VR the offset is the
    // TRUE per-eye delta from `locate_views` (a full 3D vector); on flatscreen stereo it is the
    // synthetic +/- half-IPD along the camera right axis. Either way, re-derive m_View from the moved
    // m_TransformF and rebuild the view-projections with the engine's own Multiply4x4, so the offset
    // reaches the full-m_ViewProjection shaders (transparents/sky/water), not just the
    // camera-relative opaque path.
    if is_render_camera && stereo_active {
        if let Some(vr) = vr_eye {
            unsafe {
                if let Some(camera) = camera.as_mut() {
                    // Fallback convention: write the already-reverse-Z'd off-axis projection now
                    // (after SetupRenderCamera, so the engine does not re-reverse it); the VP rebuild
                    // below picks it up. TAA jitter is not applied on this path.
                    if vr.convention == crate::vr::ProjectionConvention::ManualReverseZ {
                        camera.m_Projection.data = vr.projection_reverse_z;
                        camera.m_ProjectionF.data = vr.projection_reverse_z;
                    }
                    let delta = vr.world_offset;
                    camera.m_TransformF.data[12] += delta.x;
                    camera.m_TransformF.data[13] += delta.y;
                    camera.m_TransformF.data[14] += delta.z;
                    // Rotate the camera basis to this eye's orientation (display canting) about the
                    // now-offset eye position. m_TransformF is a column-vector world transform (its
                    // columns are the basis vectors), so a head-local rotation composes on the right
                    // and leaves the translation column -- the eye position just written -- intact.
                    // Identity for parallel-panel HMDs; corrects the Valve Index's ~5°/eye cant,
                    // without which the two eyes are rotationally mismatched and will not fuse.
                    let transform = glam::Mat4::from(camera.m_TransformF)
                        * glam::Mat4::from_quat(vr.orientation_delta);
                    camera.m_TransformF.data = transform.to_cols_array();
                    // Re-derive m_View = inverse(m_TransformF); the engine's data reads straight into
                    // glam's column-major matrix (see the Matrix4 glam bridge), so the inverse is
                    // written straight back.
                    let view = transform.inverse();
                    camera.m_View.data = view.to_cols_array();
                    let view = &camera.m_View as *const Matrix4;
                    let proj = &camera.m_Projection as *const Matrix4;
                    let proj_f = &camera.m_ProjectionF as *const Matrix4;
                    Matrix4::Multiply4x4(view, proj, &mut camera.m_ViewProjection);
                    Matrix4::Multiply4x4(view, proj_f, &mut camera.m_ViewProjectionF);
                    // Publish this eye's world offset for the sun-shadow cascade correction (see
                    // SetGlobalShaderConstants hook / stereo::StereoState::shadow_anchor_delta).
                    crate::stereo::STEREO_STATE.lock().shadow_anchor_delta =
                        [delta.x, delta.y, delta.z];
                }
            }
        } else if stereo_cameras {
            unsafe {
                if let Some(camera) = camera.as_mut() {
                    let eye1 = crate::stereo::draw_index() == 1;
                    let half_ipd = ipd * 0.5;
                    // Eye 0 is the LEFT eye (shift -right), eye 1 the RIGHT (shift +right), so view 0
                    // == left (OpenXR convention). Previously reversed, which made the debug pair
                    // fuse cross-eyed when the "parallel" toggle was off.
                    let offset = if eye1 { half_ipd } else { -half_ipd };
                    let delta = [
                        offset * camera.m_TransformF.data[0],
                        offset * camera.m_TransformF.data[1],
                        offset * camera.m_TransformF.data[2],
                    ];
                    camera.m_TransformF.data[12] += delta[0];
                    camera.m_TransformF.data[13] += delta[1];
                    camera.m_TransformF.data[14] += delta[2];
                    // Publish this eye's world offset for the sun-shadow cascade correction (see
                    // SetGlobalShaderConstants hook / stereo::StereoState::shadow_anchor_delta).
                    crate::stereo::STEREO_STATE.lock().shadow_anchor_delta = delta;

                    // m_View == inverse(m_TransformF), so the +offset*right shift of the camera world
                    // position is a -offset shift of the view translation-X (data[12]).
                    // SetupRenderCamera has already applied reverse-Z + jitter to the projections, so
                    // rebuild m_ViewProjection / m_ViewProjectionF from them with the engine's own
                    // Multiply4x4 (the same call it uses), sidestepping any matrix-convention
                    // guesswork.
                    camera.m_View.data[12] -= offset;
                    let view = &camera.m_View as *const Matrix4;
                    let proj = &camera.m_Projection as *const Matrix4;
                    let proj_f = &camera.m_ProjectionF as *const Matrix4;
                    Matrix4::Multiply4x4(view, proj, &mut camera.m_ViewProjection);
                    Matrix4::Multiply4x4(view, proj_f, &mut camera.m_ViewProjectionF);
                }
            }
        }
    }

    // Snapshot the final view-projection this dispatch rasterizes with (jitter and eye offset
    // applied). The FSR motion-vector correction inverts it to reconstruct each pixel's position and
    // re-anchor the velocity reprojection at this eye's own previous pose.
    if is_render_camera && let Some(camera) = unsafe { camera.as_ref() } {
        let mut stereo = crate::stereo::STEREO_STATE.lock();
        let index = stereo.draw_index;
        stereo.vp_history.cur_eye[index] = Some(glam::Mat4::from(camera.m_ViewProjectionF));
    }

    result
}

/// The camera pipeline within a frame (see `docs/engine/rendering.md` §2.2): `CameraTree::
/// UpdateRenderContexts` fills the control contexts, `PushRenderContext` copies the next render
/// context's transform into the active camera's `m_TransformT0` and `m_TransformT1` (running it
/// through a rotation jitter filter that snaps sub-epsilon deltas), and `Camera::UpdateRender`
/// lerps T0 → T1 by `dtf` into `m_TransformF` and derives `m_View = inverse(m_TransformF)`.
///
/// Both mod writes therefore happen *before* the original call: post-call writes land after the
/// Lerp and view derivation, do nothing for the current frame, and are clobbered by the next
/// frame's `PushRenderContext`. Writing pre-call also bypasses the jitter filter (which otherwise
/// quantizes small headpose rotations into visible steps), and giving T0 the previous-tick pose
/// and T1 the current one hands the engine's own dtf Lerp the pair it needs to smooth the
/// tick-rate headpose across rendered frames.
#[detour(address = jc3gi::camera::camera::Camera::UpdateRender_ADDRESS)]
fn camera_update_render(camera: *mut Camera, dt: f32, dtf: f32) {
    unsafe {
        if let Some(local_character) = Character::GetLocalPlayerCharacter().as_mut()
            && let Some(camera) = camera.as_mut()
            && let Some(cm) = CameraManager::get()
            && cm.m_ActiveCamera == camera
        {
            LAST_DTF.store(dtf.to_bits(), Ordering::Relaxed);

            // Publish the engine's own near/far for the active camera so the reconstruction override
            // can recognize the main-view depth passes by their planes. The engine sets a runtime far
            // that differs from the `Camera` constructor default, so keying that override on a
            // hardcoded plane silently misses every main pass (see `main_camera_planes`).
            *MAIN_CAMERA_PLANES.lock() = Some((camera.m_Near, camera.m_Far));

            let camera_settings = Config::lock_query(|c| c.camera);
            if !camera_settings.enabled {
                CAMERA_UPDATE_RENDER.get().unwrap().call(camera, dt, dtf);
                return;
            }

            // The headpose path needs a valid anchor; until one exists (loading screens), fall
            // back to the translation-only bone-derived placement below.
            let headpose_active =
                crate::headpose::is_active() && crate::headpose::anchor().is_some();

            if headpose_active {
                // Both position and orientation come from the tick-spaced pose pair, so the
                // engine's dtf Lerp smooths the whole camera placement — the bone reads
                // (`GetSafeBoneMatrix`) only carry the finalized sim-rate pose, and placing T0/T1
                // from them stepped the camera at the tick rate even though the mesh itself
                // interpolates via the skinning-palette pose pair.
                let cur = crate::headpose::query();
                let prev = crate::headpose::query_prev();
                let character_t1_matrix = glam::Mat4::from(local_character.m_WorldMatrixT1);
                write_camera_transform(
                    &mut camera.m_TransformT1,
                    cur.orientation,
                    camera_position(&cur, character_t1_matrix, &camera_settings),
                );
                // Republish the transform for the sim-phase readers (see the GetCameraMatrix
                // hook below).
                *LAST_CAMERA_WORLD.lock() = Some(camera.m_TransformT1.data);

                if camera_settings.always_use_t1 {
                    camera.m_TransformT0 = camera.m_TransformT1;
                } else {
                    let character_t0_matrix = glam::Mat4::from(local_character.m_WorldMatrixT0);
                    write_camera_transform(
                        &mut camera.m_TransformT0,
                        prev.orientation,
                        camera_position(&prev, character_t0_matrix, &camera_settings),
                    );
                }
            } else {
                let head_matrix = head_matrix(local_character);
                let (left_eye_matrix, right_eye_matrix) = eye_matrices(local_character);
                let character_t1_matrix = glam::Mat4::from(local_character.m_WorldMatrixT1);
                let head_position = calculate_head_position(
                    character_t1_matrix,
                    head_matrix,
                    left_eye_matrix,
                    right_eye_matrix,
                    camera_settings.use_eye_matrices,
                    &camera_settings,
                );
                camera.m_TransformT1.data[12] = head_position.x;
                camera.m_TransformT1.data[13] = head_position.y;
                camera.m_TransformT1.data[14] = head_position.z;

                if camera_settings.always_use_t1 {
                    camera.m_TransformT0 = camera.m_TransformT1;
                } else {
                    let character_t0_matrix = glam::Mat4::from(local_character.m_WorldMatrixT0);
                    let head_position_t0 = calculate_head_position(
                        character_t0_matrix,
                        head_matrix,
                        left_eye_matrix,
                        right_eye_matrix,
                        camera_settings.use_eye_matrices,
                        &camera_settings,
                    );
                    camera.m_TransformT0.data[12] = head_position_t0.x;
                    camera.m_TransformT0.data[13] = head_position_t0.y;
                    camera.m_TransformT0.data[14] = head_position_t0.z;
                }
            }
        }
    }

    CAMERA_UPDATE_RENDER.get().unwrap().call(camera, dt, dtf);
}

/// The camera position for a headpose: a head-frame eye arm pivoted about the *neck* (the pose
/// position shifted by the head-to-neck delta), plus the body-frame offset. Pivoting at the neck
/// makes pitching the head swing the eyes forward over the chest — looking down clears the body
/// instead of rotating in place at the skull base and staring into the neck.
///
/// With `use_eye_matrices` on, the arm's base is the *measured* neck-to-eye-midpoint arm from the
/// animated eye bones, and `head_offset` is a correction on top of it; with it off, `head_offset`
/// is the whole arm. The body-frame offset uses the character matrix matching the pose's side of
/// the T0/T1 pair.
fn camera_position(
    pose: &crate::headpose::HeadPose,
    character_matrix: glam::Mat4,
    camera_settings: &crate::config::CameraConfig,
) -> glam::Vec3 {
    let (_, character_rotation, _) = character_matrix.to_scale_rotation_translation();
    let neck_pivot = pose.position + crate::headpose::neck_delta();
    let eye_arm = if camera_settings.use_eye_matrices {
        crate::headpose::eye_arm()
    } else {
        glam::Vec3::ZERO
    };
    neck_pivot
        + pose.orientation * (eye_arm + camera_settings.head_offset)
        + character_rotation * camera_settings.body_offset
}

/// Write a full world transform (rotation + translation) into a camera transform slot.
fn write_camera_transform(target: &mut Matrix4, orientation: glam::Quat, position: glam::Vec3) {
    let world = glam::Mat4::from_rotation_translation(orientation, position);
    let m = world.to_cols_array();
    // Write rotation + translation (data[0..=14]); leave the projective row untouched.
    for (i, &v) in m.iter().enumerate().take(15) {
        target.data[i] = v;
    }
}

/// The sim-phase camera matrix reader: `m_NextCameraContext`'s transform, which the game's
/// sim-phase camera update rewrites from its internal camera *after* the mod's render-phase
/// context patch. With the look input consumed by the headpose, that internal camera's yaw is
/// frozen, so every sim-side reader — the player aim control's raycasts and adjusted camera
/// matrix, the weapon aim-target queries, melee and grapple tasks — aimed at a fixed world
/// direction regardless of where the head looked. Overriding the getter's output hands them the
/// same transform the render camera uses.
#[detour(
    address = jc3gi::camera::game_camera_manager::GameCameraManager::GetCameraMatrix_ADDRESS
)]
fn game_camera_manager_get_camera_matrix(manager: *const GameCameraManager, matrix: *mut Matrix4) {
    GAME_CAMERA_MANAGER_GET_CAMERA_MATRIX
        .get()
        .unwrap()
        .call(manager, matrix);
    if !crate::headpose::is_active() {
        return;
    }
    if let Some(data) = *LAST_CAMERA_WORLD.lock()
        && let Some(matrix) = unsafe { matrix.as_mut() }
    {
        matrix.data = data;
    }
}

/// The last `dtf` seen by the active camera's `UpdateRender`, as bits (see [`last_dtf`]).
static LAST_DTF: AtomicU32 = AtomicU32::new(0);

/// The camera world transform last written to the active camera's `m_TransformT1` with the
/// headpose active, republished by [`game_camera_manager_get_camera_matrix`] so sim-phase readers
/// see the headpose camera. `None` until the camera hook first runs with the headpose active.
static LAST_CAMERA_WORLD: Mutex<Option<[f32; 16]>> = Mutex::new(None);

/// The active (main) camera's near/far clip planes (`m_Near`/`m_Far`), captured each frame from the
/// engine. The reconstruction override ([`crate::hooks::graphics_engine::reconstruction`]) keys on
/// these to recognize the main-view depth passes -- whose off-axis inverse it must substitute -- and
/// skip auxiliary cameras (reflections) with different planes. The engine writes a runtime far that
/// differs from the `Camera` constructor default (~40 km vs the constructor's 38.4 km), so gating that
/// override on a hardcoded config plane misses every main pass. `None` until the camera hook first runs.
static MAIN_CAMERA_PLANES: Mutex<Option<(f32, f32)>> = Mutex::new(None);

/// The active camera's near/far as of the last `Camera::UpdateRender`, or `None` before the first.
pub fn main_camera_planes() -> Option<(f32, f32)> {
    *MAIN_CAMERA_PLANES.lock()
}

/// The single source of truth for the VR near/far planes: the engine's live active-camera planes,
/// falling back to `fallback` (the configured VR planes) only until the first camera update. The
/// per-eye projections, the cull frustum, and the reconstruction gate all resolve through here, so the
/// eyes render, cull, and reconstruct against the same planes the engine is actually using.
pub fn main_camera_planes_or(fallback: (f32, f32)) -> (f32, f32) {
    main_camera_planes().unwrap_or(fallback)
}

#[detour(address = jc3gi::camera::camera_tree::CameraTree::UpdateRenderContexts_ADDRESS)]
fn camera_tree_update_render_contexts(
    tree: *mut c_void,
    camera_control_context: *mut CameraControlContext,
) {
    CAMERA_TREE_UPDATE_RENDER_CONTEXTS
        .get()
        .unwrap()
        .call(tree, camera_control_context);

    unsafe {
        let Some(local_character) = Character::GetLocalPlayerCharacter().as_mut() else {
            return;
        };
        let Some(ccc) = camera_control_context.as_mut() else {
            return;
        };

        let camera_settings = Config::lock_query(|c| c.camera);
        if !camera_settings.enabled {
            return;
        }

        let character_t0_matrix = glam::Mat4::from(if camera_settings.always_use_t1 {
            local_character.m_WorldMatrixT1
        } else {
            local_character.m_WorldMatrixT0
        });
        let character_t1_matrix = glam::Mat4::from(local_character.m_WorldMatrixT1);

        // The previous contexts get the previous-tick headpose placement and the next contexts
        // the current one, mirroring the T0/T1 pair in `camera_update_render`; without the
        // headpose (or before a valid anchor exists), the bone-derived positions apply.
        let headpose_active = crate::headpose::is_active() && crate::headpose::anchor().is_some();
        let (previous_position, next_position, previous_orientation, next_orientation) =
            if headpose_active {
                let cur = crate::headpose::query();
                let prev = crate::headpose::query_prev();
                (
                    camera_position(&prev, character_t0_matrix, &camera_settings),
                    camera_position(&cur, character_t1_matrix, &camera_settings),
                    Some(prev.orientation),
                    Some(cur.orientation),
                )
            } else {
                let head_matrix = head_matrix(local_character);
                let (left_eye_matrix, right_eye_matrix) = eye_matrices(local_character);
                (
                    calculate_head_position(
                        character_t0_matrix,
                        head_matrix,
                        left_eye_matrix,
                        right_eye_matrix,
                        camera_settings.use_eye_matrices,
                        &camera_settings,
                    ),
                    calculate_head_position(
                        character_t1_matrix,
                        head_matrix,
                        left_eye_matrix,
                        right_eye_matrix,
                        camera_settings.use_eye_matrices,
                        &camera_settings,
                    ),
                    None,
                    None,
                )
            };

        patch_context(
            &mut ccc.m_PreviousCameraContext,
            previous_position,
            previous_orientation,
            camera_settings.blurs_enabled,
        );
        patch_context(
            &mut ccc.m_PreviousRenderContext,
            previous_position,
            previous_orientation,
            camera_settings.blurs_enabled,
        );
        patch_context(
            &mut ccc.m_NextCameraContext,
            next_position,
            next_orientation,
            camera_settings.blurs_enabled,
        );
        patch_context(
            &mut ccc.m_NextRenderContext,
            next_position,
            next_orientation,
            camera_settings.blurs_enabled,
        );
    }

    fn patch_context(
        context: &mut CameraContext,
        head_position: glam::Vec3,
        headpose_orientation: Option<glam::Quat>,
        blurs_enabled: bool,
    ) {
        if let Some(orientation) = headpose_orientation {
            let camera_world = glam::Mat4::from_rotation_translation(orientation, head_position);
            let m = camera_world.to_cols_array();
            for (i, &v) in m.iter().enumerate().take(15) {
                context.m_CameraTransform.data[i] = v;
            }
        } else {
            context.m_CameraTransform.data[12] = head_position.x;
            context.m_CameraTransform.data[13] = head_position.y;
            context.m_CameraTransform.data[14] = head_position.z;
        }
        context.m_AlternateAimTransform = context.m_CameraTransform;
        context.m_ListenerTransform = context.m_CameraTransform;
        context.m_FOV = 90.0_f32.to_radians();

        if !blurs_enabled {
            context.m_MaxMotionBlur = 0.0;
            context.m_MotionBlurFactor = 0.0_f32;
            context.m_RadialBlurFactor = 0.0;
        }
    }
}

fn head_matrix(character: &mut Character) -> glam::Mat4 {
    let mut head_matrix = Matrix4::default();
    unsafe {
        character.GetSafeBoneMatrix(SafeBoneIndex::HEAD, &mut head_matrix);
    }
    glam::Mat4::from(head_matrix)
}

fn eye_matrices(character: &mut Character) -> (glam::Mat4, glam::Mat4) {
    let mut left_eye_matrix = Matrix4::default();
    let mut right_eye_matrix = Matrix4::default();
    unsafe {
        if let Some(ac) = character.m_AnimatedModel.m_AnimationController.as_mut() {
            ac.GetBoneMatrix(
                ac.GetBoneIndex(hashlittle(b"fLeftEye") as u32),
                &mut left_eye_matrix,
            );
            ac.GetBoneMatrix(
                ac.GetBoneIndex(hashlittle(b"fRightEye") as u32),
                &mut right_eye_matrix,
            );
        }
    }
    (
        glam::Mat4::from(left_eye_matrix),
        glam::Mat4::from(right_eye_matrix),
    )
}

fn calculate_head_position(
    character_matrix: glam::Mat4,
    head_matrix: glam::Mat4,
    left_eye_matrix: glam::Mat4,
    right_eye_matrix: glam::Mat4,
    use_eye_matrices: bool,
    camera_settings: &crate::config::CameraConfig,
) -> glam::Vec3 {
    let (_, character_rotation, _character_position) =
        character_matrix.to_scale_rotation_translation();

    if use_eye_matrices {
        let left_eye_worldspace_matrix = character_matrix * left_eye_matrix;
        let (_, _, left_eye_position) = left_eye_worldspace_matrix.to_scale_rotation_translation();

        let right_eye_worldspace_matrix = character_matrix * right_eye_matrix;
        let (_, _, right_eye_position) =
            right_eye_worldspace_matrix.to_scale_rotation_translation();

        (left_eye_position + right_eye_position) / 2.0
    } else {
        let head_worldspace_matrix = character_matrix * head_matrix;
        let (_, head_rotation, mut head_position) =
            head_worldspace_matrix.to_scale_rotation_translation();

        head_position += head_rotation * camera_settings.head_offset;
        head_position += character_rotation * camera_settings.body_offset;

        head_position
    }
}
