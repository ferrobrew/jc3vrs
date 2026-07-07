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
//! off-axis projection while a VR eye is drawn, correcting every one of those passes at their shared
//! source. See [`StereoConfig::reconstruct_offaxis_inverse`](crate::config::StereoConfig).

use detours_macro::detour;
use jc3gi::types::math::Matrix4;
use re_utilities::hook_library::HookLibrary;

use crate::config::Config;

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
/// (flatscreen frames carry no render params), or the requested near/far do not match the configured
/// VR planes (an auxiliary camera -- e.g. a reflection -- whose own symmetric rebuild is already
/// correct).
fn offaxis_inverse(near: f32, far: f32) -> Option<[f32; 16]> {
    let (enabled, near_clip, far_clip) = Config::lock_query(|c| {
        (
            c.stereo.reconstruct_offaxis_inverse,
            c.vr.near_clip,
            c.vr.far_clip,
        )
    });
    if !enabled {
        return None;
    }
    let vr = crate::vr::render_params(crate::stereo::draw_index())?;
    // The off-axis projection is built with the configured near/far, which match the engine's main
    // camera planes; a call with different planes belongs to some other camera whose symmetric
    // reconstruction is correct as-is, so leave it untouched.
    if (near - near_clip).abs() > near_clip.abs().max(0.01) * 0.1
        || (far - far_clip).abs() > far_clip.abs() * 0.01
    {
        return None;
    }
    // The `Matrix4` <-> glam bridge transposes each way, so `Mat4::from(engine).inverse().to_cols_array()`
    // yields the inverse back in engine row-major format -- the same pattern the camera hook uses to
    // write `m_View`. `PerspectiveFovInverse` produces a standard-depth inverse, so invert the
    // standard-depth off-axis projection: the matching depth basis, now carrying the off-center shear
    // the symmetric rebuild omitted.
    let inverse = glam::Mat4::from(Matrix4 {
        data: vr.projection_standard,
    })
    .inverse();
    Some(inverse.to_cols_array())
}
