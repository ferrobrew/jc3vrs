//! The legacy (non-WaveWorks) water render blocks' screen-space reflection/refraction lookup, biased
//! into each eye's half of the double-wide target under the single-pass collapse.
//!
//! These blocks sample `ReflectionMap`, `RefractionMap`, and `DepthMap` through a *projective*
//! coordinate rather than `SV_Position`: their block type stages a world→screen-UV matrix on vertex
//! `cb1` once per pass, the vertex shader transforms the water vertex by it and passes the result on
//! as `TEXCOORD1`, and the pixel shader divides by `w`. The NDC→UV half-scale is already folded into
//! the CPU-side matrix, so the UV it yields is normalized over the **viewport** -- one eye's half --
//! while every buffer it indexes is the whole double-wide target. Each eye therefore reads the entire
//! two-eye image stretched across its water surface, and because the error is a fixed 2x scale it is a
//! 2x motion gain too: the reflections slide over the water as the camera moves.
//!
//! The fix is four rows of arithmetic on the matrix the type already staged --
//! `u' = (u + eye) · 0.5` -- applied around a per-eye re-issue of the block's `Draw`
//! ([`screen_uv_cb_per_eye`](crate::stereo::single_pass::screen_uv_cb_per_eye)), which is also what
//! makes the eye known. No shader is touched: the water vertex shaders take their clip position from
//! the global `cb0`, which the collapse already handles, and the projective coordinate is entirely a
//! CPU-side constant.
//!
//! Not covered: `CNvWaterHighEndRenderBlock` (the WaveWorks path used at the higher water-quality
//! settings) builds its screen UV as `SV_Position × (1/2W, 1/H)`, which is already self-consistent
//! under double-wide, and `WaterBoxRenderBlock::DrawSurface` (the surface-rendering mode), whose
//! matrix its type's `Setup` declines to stage at all.

use detours_macro::detour;
use jc3gi::graphics_engine::{
    graphics_engine::RenderContext,
    render_block::{RBIInfo, WaterBoxRenderBlock, WaterHighEndRenderBlock},
};
use re_utilities::hook_library::HookLibrary;

use crate::config::Config;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&WATER_HIGH_END_DRAW_BINDER)
        .with_static_binder(&WATER_BOX_DRAW_BINDER)
}

/// The high-end water family (`waterhighend`, `waterbelow`, `watershader_lod0/1/2`) reads its
/// screen-UV matrix from vertex `cb1` registers 1..4, staged from the render context's
/// world→clip view-projection.
#[detour(address = jc3gi::graphics_engine::render_block::WaterHighEndRenderBlock::Draw_ADDRESS)]
fn water_high_end_draw(
    this: *const WaterHighEndRenderBlock,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let detour = WATER_HIGH_END_DRAW.get().unwrap();
    // SAFETY: `rc` is the live render context the engine passed into `Draw`, read only for its
    // matrices; the closure re-invokes the original `Draw` trampoline.
    let handled = unsafe {
        per_eye(
            rc,
            HIGH_END_REGISTER,
            |rc| rc.m_ViewProjectionF.data,
            || {
                detour.call(this, rc, info);
            },
        )
    };
    if !handled {
        detour.call(this, rc, info);
    }
}

/// The water-box family (`waterbox`, `waterboxbelow`, `waterboxclear`) reads the same kind of matrix
/// from vertex `cb1` registers 4..7, staged from the translation-free view-projection instead --
/// its geometry is camera-relative.
#[detour(address = jc3gi::graphics_engine::render_block::WaterBoxRenderBlock::Draw_ADDRESS)]
fn water_box_draw(this: *const WaterBoxRenderBlock, rc: *mut RenderContext, info: *const RBIInfo) {
    let detour = WATER_BOX_DRAW.get().unwrap();
    // SAFETY: as above.
    let handled = unsafe {
        per_eye(
            rc,
            BOX_REGISTER,
            |rc| rc.m_OffsetViewProjection.data,
            || detour.call(this, rc, info),
        )
    };
    if !handled {
        detour.call(this, rc, info);
    }
}

/// The vertex constant buffer the water block types stage their screen-UV matrix into.
const CONSTANT_BUFFER: i32 = 1;

/// The first register of the high-end family's screen-UV matrix (`TypeConstants.ReflectionViewProj`).
const HIGH_END_REGISTER: u32 = 1;

/// The first register of the water-box family's screen-UV matrix (`cbWaterConsts.WaterConsts[4..7]`).
const BOX_REGISTER: u32 = 4;

/// Re-issue `draw` once per eye with the eye-half-biased screen-UV matrix, or return `false` when the
/// flag is off, the render context is unreadable, or the collapse intercept declines.
///
/// `view_projection` picks the render-context matrix the block type bakes the matrix from; this
/// recomputes the type's staged rows rather than intercepting them, because the type stages them once
/// per pass, well before the `Draw` being re-issued.
///
/// # Safety
///
/// `rc` must be the live [`RenderContext`] the detoured `Draw` received, and `draw` must invoke the
/// block's original `Draw` trampoline.
unsafe fn per_eye(
    rc: *mut RenderContext,
    reg_offset: u32,
    view_projection: impl Fn(&RenderContext) -> [f32; 16],
    draw: impl FnMut(),
) -> bool {
    if !Config::lock_query(|c| c.stereo.single_pass_water_uv_per_eye) {
        return false;
    }
    // SAFETY: `rc` is live per the caller contract.
    let Some(view_projection) = (unsafe { rc.as_ref() }).map(view_projection) else {
        return false;
    };
    // SAFETY: as above; `draw` is the block's original `Draw`.
    unsafe {
        crate::stereo::single_pass::screen_uv_cb_per_eye(
            rc,
            CONSTANT_BUFFER,
            reg_offset,
            screen_uv_matrix(view_projection),
            draw,
        )
    }
}

/// The rows the block type stages: `view_projection · TEX_BIAS`, in the engine's row-major storage.
///
/// Loading a row-major matrix into glam's column-major reading yields its transpose, and the
/// row-vector product `A · B` transposes to `Bᵀ · Aᵀ` -- so the factors swap and the result reads back
/// row-major unchanged.
fn screen_uv_matrix(view_projection: [f32; 16]) -> [f32; 16] {
    (glam::Mat4::from_cols_array(&TEX_BIAS) * glam::Mat4::from_cols_array(&view_projection))
        .to_cols_array()
}

/// The NDC→texture bias the water block types post-multiply their view-projection by: the standard
/// `xy · 0.5 + w · 0.5` with the depth row left alone, folded in on the CPU so the shader can divide
/// the interpolated result by `w` and sample directly. Row-major, row-vector convention (the
/// translation is the last row).
#[rustfmt::skip]
const TEX_BIAS: [f32; 16] = [
    0.5, 0.0, 0.0, 0.0,
    0.0, 0.5, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    0.5, 0.5, 0.0, 1.0,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The composed transform maps a clip position into the eye's half of the double-wide target: the
    /// screen-UV matrix's own NDC→UV bias, then the eye-half bias the per-eye re-issue applies.
    #[test]
    fn eye_half_bias_maps_ndc_into_the_eyes_half() {
        // A clip position with a non-unit `w`, to catch a bias applied to the post-divide `u` instead
        // of to the projective components.
        let clip = glam::Vec4::new(0.5, -0.25, 0.75, 2.0);
        let rows = screen_uv_matrix(glam::Mat4::IDENTITY.to_cols_array());
        let row = |k: usize| glam::Vec4::from_slice(&rows[k * 4..k * 4 + 4]);

        for eye in 0..2 {
            // The row-vector product with the per-eye bias `screen_uv_cb_per_eye` composes.
            let biased = |k: usize| {
                let r = row(k);
                glam::Vec4::new(r.x * 0.5 + r.w * 0.5 * eye as f32, r.y, r.z, r.w)
            };
            let projective: glam::Vec4 = (0..4).map(|k| clip[k] * biased(k)).sum();
            // The vertex shader packs `(x, y, w)` into `TEXCOORD1` and the pixel shader divides the
            // first two by the third.
            let uv = glam::Vec2::new(projective.x, projective.y) / projective.w;

            let ndc = glam::Vec2::new(clip.x, clip.y) / clip.w;
            let expected_u = ((ndc.x * 0.5 + 0.5) + eye as f32) * 0.5;
            assert!((uv.x - expected_u).abs() < 1e-6, "eye {eye}: {uv:?}");
            assert!(
                (uv.y - (ndc.y * 0.5 + 0.5)).abs() < 1e-6,
                "eye {eye}: {uv:?}"
            );
        }
    }
}
