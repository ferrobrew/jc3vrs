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

use std::ffi::c_void;

use detours_macro::detour;
use jc3gi::{
    camera::{camera::Camera, camera_manager::CameraManager},
    graphics_engine::{
        graphics_engine::OccluderCollectionManager, render_engine::STerrainPatchSystem,
    },
    types::math::Matrix4,
};
use re_utilities::hook_library::HookLibrary;

use crate::config::Config;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&GET_BFBC_FRUSTUM_PARAMS_BINDER)
        .with_static_binder(&TERRAIN_PATCH_SYSTEM_UPDATE_BINDER)
        .with_static_binder(&CAMERA_UPDATE_FRUSTUM_BINDER)
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

// `TerrainPatchSystemUpdate` copies the narrow centre `m_ActiveCamera` into the landscape system's own
// `m_TerrainCamera` and points every terrain render pass's frustum at it, so terrain patches cull
// against that camera's precomputed `m_FrustumPlane` -- a *separate* camera from the occluder cull
// camera the frustum widen above touches (which is why widening the occluder camera did nothing for
// terrain). Make `m_TerrainCamera` the binocular union camera after the copy, so the terrain patch set
// covers both eyes (fixes the outer-edge and bottom patch holes when flying). It runs once per frame
// and the copy reverts the camera each call it fires, so the union is re-applied unconditionally --
// affordable here (unlike the hot occluder hook) because the full frustum rebuild runs once per frame.
#[detour(address = jc3gi::graphics_engine::render_engine::TerrainPatchSystemUpdate_ADDRESS)]
fn terrain_patch_system_update(handle: *mut STerrainPatchSystem, ctx: *mut c_void) {
    TERRAIN_PATCH_SYSTEM_UPDATE.get().unwrap().call(handle, ctx);
    if !Config::lock_query(|c| c.stereo.widen_terrain_cull) {
        return;
    }
    let Some(cull) = crate::vr::cull_projection_standard() else {
        return;
    };
    // SAFETY: the original just populated the patch system; `m_TerrainCamera` is a fresh copy of the
    // active camera, so its `m_View`/`m_TransformF` are valid.
    let Some(sys) = (unsafe { handle.as_mut() }) else {
        return;
    };
    make_union_camera(&mut sys.m_TerrainCamera, cull);
}

/// Make `camera` represent the binocular union view: stamp the union projection and rebuild every
/// frustum field derived from it -- the view-projection and the six world-space frustum planes that
/// precomputed-plane cull consumers (terrain patches) read. The union projection is
/// [`crate::vr::cull_projection_standard`], synthesised from both eyes' FOVs plus the lateral IPD
/// margin. `UpdateFrustum` reads the standard-depth `m_ViewProjection`, so rebuild it from the union
/// first (`m_ViewProjection = m_View * union`).
fn make_union_camera(camera: &mut Camera, union: [f32; 16]) {
    camera.m_ProjectionF.data = union;
    let union_mat = Matrix4 { data: union };
    // Copy the transform out so the `&mut self` `UpdateFrustum` call does not alias a borrow into the
    // camera.
    let transform = camera.m_TransformF;
    unsafe {
        Matrix4::Multiply4x4(
            &raw const camera.m_View,
            &raw const union_mat,
            &raw mut camera.m_ViewProjection,
        );
        camera.UpdateFrustum(&raw const transform);
    }
}

// Camera::UpdateFrustum -- rebuilds a camera's six world-space cull frustum planes (`m_FrustumPlane`)
// from its standard-depth `m_ViewProjection`, once per frame per camera. The active camera's planes gate
// a SECOND model-visibility cull -- `CModelInstance::AddToRender` frustum-tests each render block against
// them (`CCamera::IsBoxVisible`), a gate the scene-cull widen never reaches, so large buildings pop out
// at the combined-eye edge (`docs/engine/model-culling.md`). For the active camera, rebuild its planes
// from the binocular union projection so that cull -- and the instant-hide-instead-of-fade pop, road
// meshes, and far lights that read the same planes -- covers both eyes. `m_ViewProjection` is restored
// afterwards, so the per-eye render matrices are untouched; only the cull planes widen. The rebuild goes
// through the trampoline (`original.call`), not the method binding, so it does not re-enter this detour.
#[detour(address = jc3gi::camera::camera::Camera::UpdateFrustum_ADDRESS)]
fn camera_update_frustum(this: *mut Camera, transform: *const Matrix4) {
    let original = CAMERA_UPDATE_FRUSTUM.get().unwrap();
    original.call(this, transform);
    if !Config::lock_query(|c| c.stereo.widen_model_cull) {
        return;
    }
    let Some(union) = crate::vr::cull_projection_standard() else {
        return;
    };
    // Scope the widen to the active camera -- other cameras (terrain, reflection, shadow) rebuild their
    // own planes through here and must stay narrow.
    let active = unsafe { CameraManager::get() }
        .map(|cm| cm.m_ActiveCamera)
        .unwrap_or(std::ptr::null_mut());
    if this.is_null() || this != active {
        return;
    }
    let union_mat = Matrix4 { data: union };
    // SAFETY: `this` is the live active camera on the game thread; the widen saves `m_ViewProjection`,
    // rebuilds the planes from the union, and restores it, all through raw pointers (no aliasing `&mut`
    // across the trampoline call).
    unsafe {
        let vp = &raw mut (*this).m_ViewProjection;
        let saved_vp = *vp;
        Matrix4::Multiply4x4(&raw const (*this).m_View, &raw const union_mat, vp);
        original.call(this, transform);
        *vp = saved_vp;
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
