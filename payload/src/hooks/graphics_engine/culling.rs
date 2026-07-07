//! The `OccluderCollectionManager::GetBFBCFrustumParamsForCameraAndTime` detour: widen the scene
//! cull frustum to cover both eyes in VR.
//!
//! The engine determines scene visibility (terrain, models, streaming, occlusion) once per frame in
//! the sim phase, against a single cull camera that is a verbatim copy of the active *centre* camera
//! (`GetBFBCFrustumParamsForCameraAndTime` builds the frustum from that camera's `m_View` and
//! `m_ProjectionF`). Both eyes then draw from that one center-culled visible set. In VR each eye's
//! off-axis projection reaches wider and is shifted laterally, so geometry an eye can see past the
//! center frustum's edge was never emitted -- the black voids and pop-in at the outer edges.
//!
//! This detour overwrites the cull camera's `m_ProjectionF` with a symmetric union-FOV projection
//! that bounds both eyes' frusta (built per frame in [`crate::vr::cull_projection_standard`]) just
//! before the engine builds the frustum from it, so the visible set covers everything either eye can
//! see. The per-eye *render* projections are untouched; only the cull frustum widens, and only for the
//! main-view cull camera -- shadow and other cull cameras (which pass through the same function) keep
//! their own frusta. See [`StereoConfig::widen_cull_frustum`](crate::config::StereoConfig).

use detours_macro::detour;
use jc3gi::{
    camera::{camera::Camera, camera_manager::CameraManager},
    graphics_engine::graphics_engine::OccluderCollectionManager,
    types::math::Matrix4,
};
use re_utilities::hook_library::HookLibrary;

use crate::config::Config;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&GET_BFBC_FRUSTUM_PARAMS_BINDER)
}

#[detour(
    address = jc3gi::graphics_engine::graphics_engine::OccluderCollectionManager::GetBFBCFrustumParamsForCameraAndTime_ADDRESS
)]
fn get_bfbc_frustum_params(
    this: *mut OccluderCollectionManager,
    camera: *const Camera,
    time: i32,
    out_params: *mut u64,
    out_state: *mut u64,
) {
    widen_main_cull_camera(camera);
    GET_BFBC_FRUSTUM_PARAMS
        .get()
        .unwrap()
        .call(this, camera, time, out_params, out_state);
}

/// If `camera` is the main-view cull camera and a VR frame is rendering, overwrite its `m_ProjectionF`
/// with the union-FOV projection so the frustum the engine is about to build from it covers both eyes.
/// A no-op on flatscreen (no cull projection published) or with the toggle off.
///
/// The main-view cull camera is a copy of the active centre camera, so its projection matches
/// `m_ActiveCamera.m_ProjectionF` until this widens it; that is the discriminator against the shadow
/// and other cull cameras that also flow through this function. Rewriting to the same union value is
/// idempotent, so re-entry from parallel cull jobs is harmless.
fn widen_main_cull_camera(camera: *const Camera) {
    if !Config::lock_query(|c| c.stereo.widen_cull_frustum) {
        return;
    }
    let Some(cull) = crate::vr::cull_projection_standard() else {
        return;
    };
    // SAFETY: `camera` is the live cull camera the engine passes by const reference; only its
    // projection field is written, and only when it is the main-view cull camera (checked below).
    let Some(camera) = (unsafe { (camera as *mut Camera).as_mut() }) else {
        return;
    };
    if is_main_cull_camera(&camera.m_ProjectionF) {
        camera.m_ProjectionF.data = cull;
    }
}

/// Whether `projection` is the active centre camera's projection -- i.e. this is the main-view cull
/// camera (a memcpy of the active camera), not a shadow or other cull camera.
fn is_main_cull_camera(projection: &Matrix4) -> bool {
    unsafe {
        let Some(cm) = CameraManager::get() else {
            return false;
        };
        let Some(active) = cm.m_ActiveCamera.as_ref() else {
            return false;
        };
        projections_match(&projection.data, &active.m_ProjectionF.data)
    }
}

/// Element-wise near-equality of two projection matrices. The cull camera is a byte copy of the active
/// camera, so an exact match holds until the widen; the tolerance only guards against float noise.
fn projections_match(a: &[f32; 16], b: &[f32; 16]) -> bool {
    a.iter()
        .zip(b.iter())
        .all(|(x, y)| (x - y).abs() <= 1e-4 * (1.0 + y.abs()))
}
