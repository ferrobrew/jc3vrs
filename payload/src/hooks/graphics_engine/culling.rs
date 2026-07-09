//! The `OccluderCollectionManager::GetBFBCFrustumParamsForCameraAndTime` detour: correct the scene
//! visibility system for VR, main-view only.
//!
//! The engine determines scene visibility (terrain, models, streaming, occlusion) once per frame in
//! the sim phase, against a single cull camera -- the occluder manager's own `m_CullingCamera`, a
//! verbatim copy of the active *centre* camera. Both eyes then draw from that one center-culled
//! visible set. In VR each eye's off-axis projection reaches wider and is shifted laterally, so
//! geometry an eye can see past the center's edge was never emitted -- the black voids and pop-in at
//! the outer edges.
//!
//! This detour makes three main-view-only corrections, all at the same hook. Before the engine builds
//! the frustum it overwrites the cull camera's `m_ProjectionF` with a symmetric union-FOV projection
//! that bounds both eyes' frusta (built per frame in [`crate::vr::cull_projection_standard`]), so the
//! visible set covers everything either eye can see, and relaxes the camera's `m_FOVT1` so the
//! separate *screen-space size cull* (which scales with `tan(FOV/2)` and is ~2x too aggressive under
//! the mod's injected 90 deg FOV) stops dropping small/distant geometry and vehicle sub-meshes. After
//! the engine builds the params it optionally drops the *software-occlusion* frustums (occluders cast
//! from the single centre viewpoint, wrong for two offset eyes). The per-eye *render* projections are
//! untouched, and every correction is scoped to the main cull camera by its exact identity
//! (`camera == this + 0x8`) -- shadow and reflection culls use different functions, and any other
//! camera through this hook fails the check. See
//! [`StereoConfig::widen_cull_frustum`](crate::config::StereoConfig),
//! [`cull_size_fov_deg`](crate::config::StereoConfig::cull_size_fov_deg), and
//! [`disable_bfbc_occlusion`](crate::config::StereoConfig::disable_bfbc_occlusion).
//!
//! Terrain patches are a known exception: their visibility is decided by a separate landscape system
//! that does not read this cull frustum, so these corrections do not affect terrain-patch pop-in.

use detours_macro::detour;
use jc3gi::{camera::camera::Camera, graphics_engine::graphics_engine::OccluderCollectionManager};
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
    // The main cull camera is the manager's own `m_CullingCamera` at `this + 0x8` -- the exact
    // identity every main-view consumer passes. All three corrections are scoped to it.
    let main = is_main_cull_camera(this, camera);

    // Widen the frustum (and relax the size cull) before the engine builds the params from it.
    if main {
        widen_main_cull_camera(camera);
    }

    GET_BFBC_FRUSTUM_PARAMS
        .get()
        .unwrap()
        .call(this, camera, time, out_params, out_state);

    // Drop the software-occlusion frustums after the params are built.
    if main && Config::lock_query(|c| c.stereo.disable_bfbc_occlusion) {
        unsafe { relax_main_view_occlusion(out_params) };
    }
}

/// Whether `camera` is the main-view cull camera: the occluder manager's own `m_CullingCamera`, at
/// `OccluderCollectionManager + 0x8`. Every main-view consumer (terrain, dynamic/static models,
/// streaming, AO volumes) passes exactly this camera; shadow and reflection culls use different
/// functions, and any other camera through this hook fails the check.
fn is_main_cull_camera(this: *const OccluderCollectionManager, camera: *const Camera) -> bool {
    !this.is_null() && camera as usize == this as usize + 0x8
}

/// Overwrite the (already-identified main) cull camera's `m_ProjectionF` with the union-FOV projection
/// so the frustum the engine builds covers both eyes, and relax its `m_FOVT1` so the screen-space size
/// cull stops over-dropping. A no-op on flatscreen (no cull projection published) or with the toggle
/// off. Rewriting to the same union value is idempotent, so re-entry from parallel cull jobs is
/// harmless.
fn widen_main_cull_camera(camera: *const Camera) {
    let (widen, size_fov_deg) =
        Config::lock_query(|c| (c.stereo.widen_cull_frustum, c.stereo.cull_size_fov_deg));
    if !widen {
        return;
    }
    let Some(cull) = crate::vr::cull_projection_standard() else {
        return;
    };
    // SAFETY: `camera` is the live main cull camera the engine passes by const reference; only its
    // projection and size-cull FOV fields are written.
    let Some(camera) = (unsafe { (camera as *mut Camera).as_mut() }) else {
        return;
    };
    camera.m_ProjectionF.data = cull;
    // Relax the screen-space size cull, which reads `m_FOVT1` and is otherwise ~2x too aggressive under
    // the mod's injected 90 deg FOV -- dropping small/distant geometry and vehicle sub-meshes at double
    // the distance (see `StereoConfig::cull_size_fov_deg`). `m_FOVT1` is radians, matching the injected
    // `context.m_FOV`. Only the size and AO-volume culls read it on the cull camera, so this leaves the
    // frustum cull and LOD untouched.
    if size_fov_deg > 0.0 {
        camera.m_FOVT1 = size_fov_deg.to_radians();
    }
}

/// Drop the main view's software-occlusion frustums: set `m_FrustumCount` (`+0x1280`, = occluder
/// count + 1) to 1, leaving only the widened camera frustum at index 0, so software occlusion no longer
/// culls edge geometry that the centre viewpoint hides but an offset eye can see. The occluder data
/// stays in place, just un-iterated; view-frustum culling (index 0) is unchanged.
///
/// # Safety
///
/// `out_params` is the engine's just-populated output pointer; `*out_params` addresses the per-camera
/// `SBFBCFrustumCullParameters` in the manager's cache slot, whose `m_FrustumCount` lives at `+0x1280`
/// (struct size `0x1290`). The caller copies these params only after this returns.
unsafe fn relax_main_view_occlusion(out_params: *mut u64) {
    if out_params.is_null() {
        return;
    }
    let params = unsafe { *out_params } as *mut u8;
    if params.is_null() {
        return;
    }
    let count = unsafe { params.add(0x1280).cast::<u32>() };
    let pre = unsafe { *count };
    // The count is `occluderCount + 1` with at most 32 occluder slots, so a valid value is 2..=33
    // (1 = camera frustum only, nothing to drop). Anything outside that is not a frustum count -- a
    // sign the offset is wrong -- so skip the write rather than corrupt whatever the field actually is.
    if (2..=33).contains(&pre) {
        unsafe { *count = 1 };
    }
}
