use std::ffi::c_void;

use detours_macro::detour;
use jc3gi::{
    camera::{
        camera::Camera,
        camera_context::{CameraContext, CameraControlContext},
        camera_manager::CameraManager,
    },
    character::character::{Character, SafeBoneIndex},
    graphics_engine::graphics_engine::GraphicsEngine,
    hash::hashlittle,
    types::math::Matrix4,
};
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
}

/// The scene render camera is an engine-owned copy at `GraphicsEngine + 0x170`, rebuilt each Draw by
/// `SetupRenderCamera` (reverse-Z + jitter, then `m_ViewProjection`/`m_ViewProjectionF` from
/// `m_View`). For the stereo double-Draw we offset that copy's `m_View` laterally per eye, *before*
/// the rebuild, so the two dispatches diverge. See `docs/rendering.md` section 2.
#[detour(address = jc3gi::camera::camera::Camera::SetupRenderCamera_ADDRESS)]
fn setup_render_camera(camera: *mut Camera, jitter: bool) -> *mut c_void {
    let is_render_camera = unsafe {
        GraphicsEngine::get()
            .is_some_and(|ge| (ge as *mut GraphicsEngine as usize) + 0x170 == camera as usize)
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
    let result = SETUP_RENDER_CAMERA.get().unwrap().call(camera, jitter);

    // Snapshot the center view-projection before the FSR-jitter and per-eye blocks below patch it.
    // This is the value the engine's own sim-side previous-VP snapshot holds (un-offset, unjittered
    // -- the engine jitter is disabled above whenever we patch), which the velocity pass reprojects
    // with; the FSR motion-vector correction needs it as its "what the engine encoded" matrix. The
    // full pristine matrix set is kept alongside it so the Draw driver can restore the render camera
    // after the eye loop -- the sim-side sun-shadow fit reads this camera and must see the center,
    // unjittered state or its cascade texel snap flip-flops (issue #10's blob flicker).
    if is_render_camera && let Some(camera) = unsafe { camera.as_ref() } {
        let mut stereo = crate::stereo::STEREO_STATE.lock();
        stereo.vp_history.cur_center = Some(glam::Mat4::from(camera.m_ViewProjectionF));
        stereo.pristine_render_camera = Some(crate::stereo::PristineRenderCamera {
            camera: camera as *const Camera as usize,
            matrices: [
                camera.m_Projection.data,
                camera.m_ProjectionF.data,
                camera.m_View.data,
                camera.m_TransformF.data,
                camera.m_ViewProjection.data,
                camera.m_ViewProjectionF.data,
            ],
        });
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
    // the CameraPosition the camera-relative scene render subtracts) along its right axis by +/-
    // half the IPD. Same projection both eyes -- a per-eye zoom would make the pair unfusable.
    if is_render_camera && stereo_active && stereo_cameras {
        unsafe {
            if let Some(camera) = camera.as_mut() {
                let eye1 = crate::stereo::draw_index() == 1;
                let half_ipd = ipd * 0.5;
                // Eye 0 is the LEFT eye (shift -right), eye 1 the RIGHT (shift +right), so view 0 ==
                // left (OpenXR convention). Previously reversed, which made the debug pair fuse
                // cross-eyed when the "parallel" toggle was off.
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

                // Also offset the view so the parallax reaches full-m_ViewProjection shaders
                // (transparents/sky/water), not just the camera-relative opaque path. m_View ==
                // inverse(m_TransformF), so the +offset*right shift of the camera world position is a
                // -offset shift of the view translation-X (data[12]). SetupRenderCamera has already
                // applied reverse-Z + jitter to the projections, so rebuild m_ViewProjection /
                // m_ViewProjectionF from them with the engine's own Multiply4x4 (the same call it
                // uses), sidestepping any matrix-convention guesswork.
                camera.m_View.data[12] -= offset;
                let view = &camera.m_View as *const Matrix4;
                let proj = &camera.m_Projection as *const Matrix4;
                let proj_f = &camera.m_ProjectionF as *const Matrix4;
                Matrix4::Multiply4x4(view, proj, &mut camera.m_ViewProjection);
                Matrix4::Multiply4x4(view, proj_f, &mut camera.m_ViewProjectionF);
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

#[detour(address = jc3gi::camera::camera::Camera::UpdateRender_ADDRESS)]
fn camera_update_render(camera: *mut Camera, dt: f32, dtf: f32) {
    unsafe {
        if let Some(local_character) = Character::GetLocalPlayerCharacter().as_mut()
            && let Some(camera) = camera.as_mut()
            && let Some(cm) = CameraManager::get()
            && cm.m_ActiveCamera == camera
        {
            let camera_settings = Config::lock_query(|c| c.camera);
            if !camera_settings.enabled {
                CAMERA_UPDATE_RENDER.get().unwrap().call(camera, dt, dtf);
                return;
            }

            let head_matrix = head_matrix(local_character);
            let (left_eye_matrix, right_eye_matrix) = eye_matrices(local_character);

            if !camera_settings.always_use_t1 {
                let character_t0_matrix = glam::Mat4::from(if camera_settings.always_use_t1 {
                    local_character.m_WorldMatrixT1
                } else {
                    local_character.m_WorldMatrixT0
                });
                let head_position = calculate_head_position(
                    character_t0_matrix,
                    head_matrix,
                    left_eye_matrix,
                    right_eye_matrix,
                    camera_settings.use_eye_matrices,
                    &camera_settings,
                );
                camera.m_TransformT0.data[12] = head_position.x;
                camera.m_TransformT0.data[13] = head_position.y;
                camera.m_TransformT0.data[14] = head_position.z;
            }

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
            }
        }
    }

    CAMERA_UPDATE_RENDER.get().unwrap().call(camera, dt, dtf);
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

        let head_matrix = head_matrix(local_character);
        let (left_eye_matrix, right_eye_matrix) = eye_matrices(local_character);

        let character_t0_matrix = glam::Mat4::from(if camera_settings.always_use_t1 {
            local_character.m_WorldMatrixT1
        } else {
            local_character.m_WorldMatrixT0
        });
        let character_t1_matrix = glam::Mat4::from(local_character.m_WorldMatrixT1);

        patch_context(
            &mut ccc.m_PreviousCameraContext,
            calculate_head_position(
                if camera_settings.always_use_t1 {
                    character_t1_matrix
                } else {
                    character_t0_matrix
                },
                head_matrix,
                left_eye_matrix,
                right_eye_matrix,
                camera_settings.use_eye_matrices,
                &camera_settings,
            ),
            camera_settings.blurs_enabled,
        );
        patch_context(
            &mut ccc.m_PreviousRenderContext,
            calculate_head_position(
                if camera_settings.always_use_t1 {
                    character_t1_matrix
                } else {
                    character_t0_matrix
                },
                head_matrix,
                left_eye_matrix,
                right_eye_matrix,
                camera_settings.use_eye_matrices,
                &camera_settings,
            ),
            camera_settings.blurs_enabled,
        );
        patch_context(
            &mut ccc.m_NextCameraContext,
            calculate_head_position(
                character_t1_matrix,
                head_matrix,
                left_eye_matrix,
                right_eye_matrix,
                camera_settings.use_eye_matrices,
                &camera_settings,
            ),
            camera_settings.blurs_enabled,
        );
        patch_context(
            &mut ccc.m_NextRenderContext,
            calculate_head_position(
                character_t1_matrix,
                head_matrix,
                left_eye_matrix,
                right_eye_matrix,
                camera_settings.use_eye_matrices,
                &camera_settings,
            ),
            camera_settings.blurs_enabled,
        );
    }

    fn patch_context(context: &mut CameraContext, head_position: glam::Vec3, blurs_enabled: bool) {
        context.m_CameraTransform.data[12] = head_position.x;
        context.m_CameraTransform.data[13] = head_position.y;
        context.m_CameraTransform.data[14] = head_position.z;
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
