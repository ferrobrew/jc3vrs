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
//!
//! Substituting one eye's inverse presumes the pass is drawing *for* one eye, which the single-pass
//! collapse breaks: there the fullscreen quad covers both eye halves of the double-wide target in a
//! single draw, so one basis is right for neither half and the resulting error turns with the camera.
//! [`enter_per_eye_half`] is the seam for running such a pass once per eye instead --
//! [`super::clustered_lighting`] drives it for the deferred resolve, which is where the sun shadow is
//! sampled.

use std::{
    cell::Cell,
    sync::atomic::{AtomicBool, Ordering},
};

use detours_macro::detour;

use jc3gi::{
    graphics_engine::{
        draw::SetScissorEnable,
        graphics_engine::{GraphicsEngine, HContext_t},
    },
    types::math::Matrix4,
};
use re_utilities::hook_library::HookLibrary;
use windows::Win32::{
    Foundation::RECT,
    Graphics::Direct3D11::{D3D11_VIEWPORT, ID3D11DeviceContext},
    System::Threading::{EnterCriticalSection, LeaveCriticalSection},
};

use crate::{
    config::Config,
    debug::trace::{TraceEvent, TraceState},
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&PERSPECTIVE_FOV_INVERSE_BINDER)
}

/// Restrict the reconstruction to one eye's half of the double-wide collapse target for as long as
/// the returned guard lives, so a fullscreen reconstruction pass can be run once per eye.
///
/// Under the single-pass collapse a fullscreen pass is one draw whose quad spans **both** eye halves,
/// while [`offaxis_inverse`] can only hand it one eye's basis -- so neither half reconstructs
/// correctly and the error rotates with the camera (the sun shadow, sampled over those positions,
/// slides across the screen). The fix is to run the pass twice, masked to one half each time.
///
/// The mask is a **scissor** rectangle, not a viewport: the reconstruction shaders derive their
/// G-buffer UV from the quad's own NDC (`o1 = (v0 * 0.5 + 0.5) * UVScale`), so halving the viewport
/// would stretch that UV across the eye's half and mis-sample the G-buffer, while a scissor leaves the
/// NDC-to-pixel mapping alone and only clips. The per-eye basis is then obtained by folding the
/// full-target-NDC → eye-NDC remap into the inverse itself (see [`half_target_remap`]).
///
/// Returns a guard that puts the scissor state back on every path. Only the pass's own
/// [`Matrix4::PerspectiveFovInverse`] call arms the mask, so a pass that turns out not to reconstruct
/// (an auxiliary camera, or the override disabled) leaves the frame exactly as it found it -- see
/// [`PerEyeHalf::masked`].
pub(super) fn enter_per_eye_half(eye: usize, ctx: *mut HContext_t) -> PerEyeHalf {
    let saved = with_immediate_context(|d3d| {
        let mut count = 2u32;
        let mut rects = [RECT::default(); 2];
        // SAFETY: `count` is the length of `rects`, as `RSGetScissorRects` requires.
        unsafe { d3d.RSGetScissorRects(&mut count, Some(rects.as_mut_ptr())) };
        rects
    });
    PER_EYE.set(Some(PerEyeState {
        eye,
        ctx,
        masked: false,
        saved_scissor: saved,
    }));
    PerEyeHalf(())
}

/// The scope opened by [`enter_per_eye_half`].
pub(super) struct PerEyeHalf(());

impl PerEyeHalf {
    /// Whether the pass actually reconstructed under the mask, i.e. whether this run rendered one eye's
    /// half rather than the whole target. `false` means nothing was masked and the run was identical to
    /// an un-split one, so the caller must **not** issue the second eye's run: it would draw the same
    /// full-width image twice.
    pub(super) fn masked(&self) -> bool {
        PER_EYE.get().is_some_and(|state| state.masked)
    }
}

impl Drop for PerEyeHalf {
    fn drop(&mut self) {
        if let Some(state) = PER_EYE.replace(None)
            && state.masked
        {
            // SAFETY: `ctx` is the graphics context the detoured `Draw` was running on, live for the
            // duration of the guard.
            unsafe { SetScissorEnable(state.ctx, false) };
            if let Some(saved) = state.saved_scissor {
                with_immediate_context(|d3d| {
                    // SAFETY: a two-element slice is a valid scissor-rect array.
                    unsafe { d3d.RSSetScissorRects(Some(&saved)) };
                });
            }
        }
    }
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
        *target = inverse;
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
fn offaxis_inverse(near: f32, far: f32) -> Option<Matrix4> {
    let (enabled, near_fallback, far_fallback) = Config::lock_query(|c| {
        (
            c.stereo.reconstruct_offaxis_inverse,
            c.vr.near_clip,
            c.vr.far_clip,
        )
    });
    // A per-eye half run reconstructs for *its* eye, not for the collapse's single dispatch index
    // (which is always eye 0); everything else keeps reading the dispatch's own eye.
    let half = PER_EYE.get();
    let eye = half.map_or_else(crate::stereo::draw_index, |state| state.eye);
    let params = crate::vr::render_params(eye);
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
    // The `Matrix4` <-> glam bridge transposes each way, so `Matrix4::from(Mat4::from(engine).inverse())`
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
        .then(|| params.map(|vr| glam::Mat4::from(vr.projection_reverse_z).inverse()))
        .flatten()
        .map(|inverse| match half {
            // A per-eye half run masks the pass to this eye's half of the double-wide target and folds
            // the full-target-NDC -> eye-NDC remap into the inverse, so the half reconstructs exactly.
            // If the mask cannot be applied (the bound target is not the collapse's double-wide scene
            // target), fall through to the plain inverse: the run is then indistinguishable from the
            // un-split pass, which is what [`PerEyeHalf::masked`] reports to the caller.
            Some(state) if arm_eye_half_scissor(&state) => {
                Matrix4::from(inverse * half_target_remap(state.eye))
            }
            _ => Matrix4::from(inverse),
        });

    // Record the reconstruction's live inputs so the trace can show whether the matrix (and hence the
    // reconstructed positions the sun shadow samples over) wobbles frame to frame. Only for VR-eye
    // dispatches, where `params` is populated; `record_eye` is a no-op outside an active trace.
    if let Some(vr) = params {
        let d = &vr.projection_reverse_z.data;
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

/// The state [`enter_per_eye_half`] publishes for the reconstruction that runs inside it.
#[derive(Clone, Copy)]
struct PerEyeState {
    /// The eye this run renders.
    eye: usize,
    /// The graphics context the run's draws are issued on, whose rasterizer-state key carries the
    /// scissor-enable bit.
    ctx: *mut HContext_t,
    /// Whether the mask was actually applied; see [`PerEyeHalf::masked`].
    masked: bool,
    /// The scissor rectangles found bound before the run, put back when the guard drops. `None` when
    /// the immediate context could not be reached, in which case nothing was changed either.
    saved_scissor: Option<[RECT; 2]>,
}

thread_local! {
    /// The per-eye half in flight on this thread, or `None` outside one. Thread-local because the
    /// re-issue and the `PerspectiveFovInverse` call it brackets both run on the render thread, and a
    /// reconstruction on any other thread must not pick up the mask.
    static PER_EYE: Cell<Option<PerEyeState>> = const { Cell::new(None) };
}

/// The full-target-NDC → eye-NDC remap for `eye`'s half of the double-wide collapse target.
///
/// The fullscreen quad spans the whole target, so its NDC `x ∈ [-1, 1]` covers both eyes; eye `e`'s
/// half is `x ∈ [-1 + e, e]`, which maps to that eye's own `x' ∈ [-1, 1]` as `x' = 2x + 1 - 2e`. Folded
/// into the clip→view inverse (applied *before* it, so it is a right factor in glam's column-vector
/// convention), it makes the pass's own untouched geometry reconstruct that eye's frustum.
fn half_target_remap(eye: usize) -> glam::Mat4 {
    glam::Mat4::from_cols(
        glam::Vec4::new(2.0, 0.0, 0.0, 0.0),
        glam::Vec4::Y,
        glam::Vec4::Z,
        glam::Vec4::new(1.0 - 2.0 * eye as f32, 0.0, 0.0, 1.0),
    )
}

/// Clip the rest of this per-eye run to `state.eye`'s half of the double-wide target, by setting both
/// scissor rectangles to that half and raising the context's scissor-enable bit. Reports whether the
/// mask was applied; it is not when the immediate context is unreachable, when no render size is known
/// yet, or when the bound viewport is not the double-wide scene target (a reduced-resolution or
/// auxiliary pass, which must be left alone).
///
/// Idempotent within a run: the second call finds the mask already up and reports success without
/// touching the device.
fn arm_eye_half_scissor(state: &PerEyeState) -> bool {
    if state.masked {
        return true;
    }
    let Some((width, _)) = crate::stereo::render_size() else {
        return false;
    };
    let Some(full) = with_immediate_context(|d3d| {
        let mut count = 1u32;
        let mut viewports = [D3D11_VIEWPORT::default(); 1];
        // SAFETY: `count` is the length of `viewports`, as `RSGetViewports` requires.
        unsafe { d3d.RSGetViewports(&mut count, Some(viewports.as_mut_ptr())) };
        viewports[0]
    }) else {
        return false;
    };
    // The eye halves are derived from this viewport, so it has to be the collapse's full double-wide
    // one -- a pass that binds anything else is not the one we can split. A pixel of slack for the
    // engine's own rounding, matching the scene-size check the collapse's viewport routing uses.
    if (full.Width - width as f32).abs() > 1.0 {
        warn_unmasked(full.Width, width);
        return false;
    }
    let half = full.Width / 2.0;
    let left = full.TopLeftX + state.eye as f32 * half;
    let rect = RECT {
        left: left as i32,
        top: full.TopLeftY as i32,
        right: (left + half) as i32,
        bottom: (full.TopLeftY + full.Height) as i32,
    };
    // Both slots: a viewport-routed shader picks its scissor rectangle by the same index it picks its
    // viewport with, and the fullscreen quad's shader writes no index at all, so the two must agree.
    let bound = with_immediate_context(|d3d| {
        // SAFETY: a two-element slice is a valid scissor-rect array.
        unsafe { d3d.RSSetScissorRects(Some(&[rect, rect])) };
    })
    .is_some();
    if !bound {
        return false;
    }
    // SAFETY: `ctx` is the graphics context the bracketed pass draws on, live for the guard's scope.
    unsafe { SetScissorEnable(state.ctx, true) };
    PER_EYE.set(Some(PerEyeState {
        masked: true,
        ..*state
    }));
    if !ENGAGED.swap(true, Ordering::Relaxed) {
        tracing::info!(
            target: "single_pass",
            "per-eye reconstruction engaged: fullscreen reconstruction passes now run once per eye, \
             scissor-masked to each half of the {width}px-wide collapse target",
        );
    }
    true
}

/// Warn, once, that a per-eye reconstruction run found a viewport it could not split, so the pass ran
/// whole-target as it does with the split off.
fn warn_unmasked(viewport_width: f32, render_width: u32) {
    if !UNMASKED_WARNED.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            target: "single_pass",
            "per-eye reconstruction declined: the bound viewport is {viewport_width}px wide, not the \
             {render_width}px double-wide scene target, so the pass ran across the whole target with \
             one eye's basis",
        );
    }
}

/// One-shot latches for the two coverage log lines above: the split is either working for the whole
/// session or not, so reporting it once is the whole signal and a per-frame line would be noise.
static ENGAGED: AtomicBool = AtomicBool::new(false);
static UNMASKED_WARNED: AtomicBool = AtomicBool::new(false);

/// Run `f` on the engine's immediate D3D context under the context mutex every other path in the mod
/// that touches it also takes. `None` when the device or context is not live yet.
///
/// The context is borrowed rather than cloned: an `AddRef`/`Release` pair per call would be wasted on
/// a path that runs a handful of times per frame and never outlives the engine.
fn with_immediate_context<R>(f: impl FnOnce(&ID3D11DeviceContext) -> R) -> Option<R> {
    // SAFETY: called on the render thread, where the engine's device/context pointers are stable.
    unsafe {
        let context = GraphicsEngine::get()?
            .m_Device
            .as_ref()?
            .m_Context
            .as_ref()?;
        EnterCriticalSection(context.m_Mutex);
        let result = f(&context.m_Context);
        LeaveCriticalSection(context.m_Mutex);
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr::projection::{Fov, OffAxisProjection};

    /// The remap must send each eye's half of the full target's NDC onto that eye's own full NDC
    /// range, and leave the other components alone.
    #[test]
    fn half_target_remap_maps_each_half_onto_the_eye_range() {
        for (eye, half) in [(0, (-1.0, 0.0)), (1, (0.0, 1.0))] {
            let remap = half_target_remap(eye);
            for (x_full, x_eye) in [
                (half.0, -1.0),
                ((half.0 + half.1) / 2.0, 0.0),
                (half.1, 1.0),
            ] {
                let mapped = remap * glam::Vec4::new(x_full, 0.25, 0.5, 1.0);
                assert!(
                    (mapped.x - x_eye).abs() < 1e-6,
                    "eye {eye}: full-target x {x_full} mapped to {} not {x_eye}",
                    mapped.x,
                );
                assert_eq!((mapped.y, mapped.z, mapped.w), (0.25, 0.5, 1.0));
            }
        }
    }

    /// Reconstructing a point of an eye's half through the remapped inverse must land where
    /// reconstructing the same point through that eye's own unremapped inverse does -- the property the
    /// whole split rests on.
    #[test]
    fn remapped_inverse_reconstructs_the_eye_the_half_belongs_to() {
        let fov = Fov {
            left: -50.0_f32.to_radians(),
            right: 40.0_f32.to_radians(),
            up: 45.0_f32.to_radians(),
            down: -45.0_f32.to_radians(),
        };
        let inverse =
            glam::Mat4::from(OffAxisProjection::new(fov, 0.1, 38400.0).reverse_z).inverse();

        for eye in 0..2 {
            let remapped = inverse * half_target_remap(eye);
            for x_eye in [-1.0, -0.5, 0.0, 0.5, 1.0] {
                // The full-target NDC x of the same point of eye `eye`'s half.
                let x_full = (x_eye - 1.0 + 2.0 * eye as f32) / 2.0;
                let through_half = remapped * glam::Vec4::new(x_full, 0.3, 0.0, 1.0);
                let direct = inverse * glam::Vec4::new(x_eye, 0.3, 0.0, 1.0);
                // The far plane sits 38.4 km out, so compare the reconstructed rays relatively: an
                // absolute tolerance there would be measuring float spacing, not agreement.
                let (through_half, direct) = (through_half / through_half.w, direct / direct.w);
                assert!(
                    (through_half - direct).length() <= 1e-4 * direct.length().max(1.0),
                    "eye {eye} at NDC x {x_eye}: {through_half:?} vs {direct:?}",
                );
            }
        }
    }
}
