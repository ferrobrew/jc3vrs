//! The `CMatrix4f::PerspectiveFovInverse` detour: rebuild the screen-space reconstruction basis from
//! the true per-eye off-axis projection while rendering a VR eye.
//!
//! The deferred and screen-space passes (SSR, deferred clustered lighting, SSAO, screen-space
//! subsurface, atmospheric scattering, depth of field) recover a clip-to-view inverse by rebuilding
//! it from a vertical field of view and an aspect ratio via
//! [`Matrix4::PerspectiveFovInverse`](jc3gi::types::math::Matrix4), then multiply by the render
//! context's camera transform to reach clip-to-world. That rebuild can only encode a *symmetric*
//! frustum. In flatscreen stereo both eyes keep the game's symmetric center projection, so the
//! rebuild is exact; in VR the mod replaces the projection with an off-center (asymmetric) per-eye
//! matrix whose shear is mirror-opposite between the two eyes, so the symmetric rebuild is wrong --
//! oppositely per eye -- and view-dependent shading (specular and reflections on car paint, chrome,
//! metal) diverges grossly between the eyes. This detour substitutes the exact inverse of the eye's
//! off-axis projection while a VR eye is drawn, correcting those passes at their shared source --
//! including the atmospheric-scattering / aerial-perspective pass, which reconstructs the whole screen
//! and samples the sun shadow cascade over it (with the depth basis correct, the off-axis inverse
//! reconstructs the sky and distant terrain without swimming).
//!
//! See [`StereoConfig::reconstruct_offaxis_inverse`](crate::config::StereoConfig).

use detours_macro::detour;

use jc3gi::types::math::Matrix4;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    debug::trace::{TraceEvent, TraceState},
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&PERSPECTIVE_FOV_INVERSE_BINDER)
}

#[detour(address = jc3gi::types::math::Matrix4::PerspectiveFovInverse_ADDRESS)]
fn perspective_fov_inverse(
    out: *mut Matrix4,
    fov: f32,
    aspect: f32,
    far: f32,
    near: f32,
) -> *mut Matrix4 {
    if let Some(inverse) = offaxis_inverse(near, far)
        && let Some(target) = unsafe { out.as_mut() }
    {
        target.data = inverse;
        return out;
    }
    PERSPECTIVE_FOV_INVERSE
        .get()
        .unwrap()
        .call(out, fov, aspect, far, near)
}

/// The engine-format inverse of the off-axis projection for the VR eye currently being drawn, or
/// `None` when the override does not apply: the toggle is off, this is not a VR eye dispatch
/// (flatscreen frames carry no render params), or the requested near/far do not match the *live*
/// main-camera planes (an auxiliary camera -- e.g. a reflection -- whose own symmetric rebuild is
/// already correct).
fn offaxis_inverse(near: f32, far: f32) -> Option<[f32; 16]> {
    let (enabled, near_fallback, far_fallback) = Config::lock_query(|c| {
        (
            c.stereo.reconstruct_offaxis_inverse,
            c.vr.near_clip,
            c.vr.far_clip,
        )
    });
    let params = crate::vr::render_params(crate::stereo::draw_index());
    // Recognize the main-view depth passes by the engine's ACTUAL active-camera planes, the single
    // source of truth ([`crate::hooks::camera::main_camera_planes_or`]), not a hardcoded config value:
    // the engine writes a runtime far (~40 km) that differs from the constructor default the config
    // mirrors (38.4 km), so comparing against the config rejected every main pass and the off-axis
    // inverse never engaged. A pass whose near/far differ from the live main camera belongs to another
    // camera (e.g. a reflection) whose symmetric rebuild is already correct, so leave it untouched.
    let (near_ref, far_ref) =
        crate::hooks::camera::main_camera_planes_or((near_fallback, far_fallback));
    let near_ok = (near - near_ref).abs() <= near_ref.abs().max(0.01) * 0.1;
    let far_ok = (far - far_ref).abs() <= far_ref.abs() * 0.01;

    let applies = enabled && near_ok && far_ok;
    // The `Matrix4` <-> glam bridge transposes each way, so `Mat4::from(engine).inverse().to_cols_array()`
    // yields the inverse back in engine row-major format -- the same pattern the camera hook uses to
    // write `m_View`. The engine's `PerspectiveFovInverse` (0x1400390E0) produces a REVERSE-Z clip->view
    // inverse (its depth entries `m23 = (far-near)/(far*near)`, `m33 = 1/far`, `m32 = -1` reconstruct
    // near->NDC z 1, far->0), matching the reverse-Z depth buffer the game renders. So invert the
    // reverse-Z off-axis projection, not the standard-depth one: `inverse(projection_reverse_z)`
    // reproduces the engine's inverse exactly for a symmetric frustum and adds the off-center shear the
    // symmetric rebuild omits. Inverting `projection_standard` (the earlier code) matched the x/y and
    // shear -- so specular/SSR looked fixed -- but sign-flipped and mis-scaled the depth basis, so the
    // sun shadow sampled over the reconstructed positions swam with the off-axis shear as the camera
    // rotated (issue #31).
    let result = applies
        .then(|| {
            params.map(|vr| {
                glam::Mat4::from(Matrix4 {
                    data: vr.projection_reverse_z,
                })
                .inverse()
                .to_cols_array()
            })
        })
        .flatten();

    // Record the reconstruction's live inputs so the trace can show whether the matrix (and hence the
    // reconstructed positions the sun shadow samples over) wobbles frame to frame. Only for VR-eye
    // dispatches, where `params` is populated; `record_eye` is a no-op outside an active trace.
    if let Some(vr) = params {
        let d = vr.projection_reverse_z;
        TraceState::record_eye(TraceEvent::ReconstructionState {
            req_near: near,
            req_far: far,
            ref_near: near_ref,
            ref_far: far_ref,
            applied: result.is_some(),
            proj: [d[0], d[5], d[8], d[9], d[10]],
        });
    }

    result
}
