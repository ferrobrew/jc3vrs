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
//! The WaveWorks family (`NvWater*`, [`NvWaterHighEndRenderBlock`]) does not have that defect -- its
//! shaders build the screen UV as `SV_Position × (1/2W, 1/H)`, which is already self-consistent under
//! double-wide -- but it has the other one, and the second half of this module fixes it: the whole
//! family takes its clip position from a model-view-projection the block bakes into its own constant
//! buffer, so the collapse's per-eye machinery never reaches it and both eyes see the collapsed centre
//! view of the water surface. See [`nv_water_per_eye`].
//!
//! Not covered: the water-box *surface* geometry (`WaterBoxRenderBlock::DrawSurface`, and the
//! `NWater::DrawWaterBoxSurface` loop [`NvWaterHighEndRenderBlock::Draw`] runs over every registered
//! box). Neither of the two mechanisms above reaches it, and neither is its defect. Its vertex shader
//! (`waterboxsurface`) builds clip as
//!
//! ```text
//! world_rel = box_transform(cb1[0..3]) · position     // scale by half-extents + (centre - camera)
//! clip      = cb0[0..3] · (world_rel + cb0[4])        // full view-projection · absolute world
//! ```
//!
//! -- the *full*, translation-bearing view-projection at global rows `0..3`, which the collapse's
//! per-eye register remap does not cover (it claims only `cb0[4]` and `cb0[29..32]`). The remap does
//! claim the shader, on that lone `cb0[4]` camera-position reference, and retargets it to `cb13`
//! while leaving the projection centred -- so the eye offset is added to the *world position* and
//! then viewed from the centre, which displaces the surface by the eye offset in the wrong direction
//! instead of giving it parallax. The per-eye re-issue below draws with one instance, so the parity
//! resolves to eye 0 in both halves and both eyes get eye 0's displacement.
//!
//! This is the whole legacy family's idiom, not one permutation's: `waterbox`, `waterboxbelow`,
//! `watershader_lod0`, and `watershader_lod1` read the same rows. The transform they want is the
//! reprojection rewrite, which replaces the clip position wholesale and so does not care that the
//! source was `cb0[0..3]` -- but reprojecting them moves where they rasterize, which invalidates the
//! projective screen UV the first half of this module corrects (that fix is deliberately *not*
//! reprojected, precisely because the geometry still lands at the centre view). The two are one
//! change, and the surface grid additionally has no per-eye re-issue of its own to hang it off.
//! See `docs/mod/single-pass-stereo.md`.

use std::{
    ffi::c_void,
    sync::atomic::{AtomicBool, Ordering},
};

use detours_macro::detour;
use jc3gi::{
    graphics_engine::{
        graphics_engine::RenderContext,
        render_block::{
            NvWaterHighEndRenderBlock, RBIInfo, WaterBoxRenderBlock, WaterHighEndRenderBlock,
        },
        render_engine::RenderPassId,
    },
    types::math::Matrix4,
};
use re_utilities::hook_library::HookLibrary;

use crate::config::Config;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&WATER_HIGH_END_DRAW_BINDER)
        .with_static_binder(&WATER_BOX_DRAW_BINDER)
        .with_static_binder(&NV_WATER_HIGH_END_DRAW_BINDER)
        .with_static_binder(&WAVE_WORKS_SIMULATION_STEP_BINDER)
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

/// The WaveWorks family, re-issued once per eye with that eye's camera substituted into the render
/// context the block bakes its matrices from. See [`nv_water_per_eye`].
#[detour(address = jc3gi::graphics_engine::render_block::NvWaterHighEndRenderBlock::Draw_ADDRESS)]
fn nv_water_high_end_draw(
    this: *const NvWaterHighEndRenderBlock,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let detour = NV_WATER_HIGH_END_DRAW.get().unwrap();
    // SAFETY: `this` and `rc` are the live block and render context the engine passed into `Draw`; the
    // closure re-invokes the original `Draw` trampoline.
    let handled = unsafe {
        nv_water_per_eye(this, rc, || {
            detour.call(this, rc, info);
        })
    };
    if !handled {
        detour.call(this, rc, info);
    }
}

/// The WaveWorks simulation step, suppressed on the second eye of an [`nv_water_per_eye`] re-issue.
///
/// Re-issuing the block's `Draw` re-drives everything in it, and this one call is the only part that
/// is not idempotent: it advances the simulation clock, blocks on the readback staging cursor, and
/// archives another displacement snapshot into the ring the CPU-side wave-height and buoyancy queries
/// read. Running it twice per frame would halve that ring's time span and pay the stall twice, for a
/// second kick at the same simulation time that produces the same water.
#[detour(address = jc3gi::graphics_engine::render_block::WaveWorksSimulationStep_ADDRESS)]
extern "system" fn wave_works_simulation_step(
    render_time: f64,
    gfx_context: *mut c_void,
    kick_id: *mut u64,
    simulation: *mut c_void,
    savestate: *mut c_void,
) {
    if SUPPRESS_SIMULATION_STEP.load(Ordering::Relaxed) {
        return;
    }
    WAVE_WORKS_SIMULATION_STEP.get().unwrap().call(
        render_time,
        gfx_context,
        kick_id,
        simulation,
        savestate,
    );
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

/// Re-issue the WaveWorks block's `Draw` once per eye with that eye's camera, or return `false` when
/// the flag is off, the pass is one of the block's auxiliary passes, the per-eye camera is
/// unavailable, or the collapse intercept declines.
///
/// Every `NvWater*` permutation writes clip position from a `g_ModelViewProjectionMatrix` at the
/// block type's *own* vertex/domain `cb1` registers 0..3, and its hull and domain shaders carry the
/// same epilogue -- none of them touch the global `cb0` the collapse's shader rewrite works on, so no
/// amount of `cb13`/viewport routing reaches them. The matrix is built in
/// [`NvWaterHighEndRenderBlock::Setup`] from exactly two render-context fields, `m_View` and
/// `m_ProjectionF`, and those same two matrices are also handed to WaveWorks itself as the view and
/// projection its quadtree culls and picks patch LODs against.
///
/// So rather than reproject a constant in flight, this substitutes the *inputs*: it writes the eye's
/// view and projection into the render context, calls the block's own `Setup` to rebuild and re-upload
/// everything downstream of them, and re-issues `Draw` -- once per eye, each into that eye's half of
/// the double-wide target. The render context is restored afterwards.
///
/// What that does *not* restore is the block's own cached matrices and the shared constant buffer,
/// which are left holding the right eye's. Nothing reads them before the next `Setup`: the draw-list
/// walk calls `Setup` before the first `Draw` of a block-type run and again whenever the sort id
/// changes, and every `Draw` in between comes back through here and restages per eye anyway.
///
/// # Safety
///
/// `this` and `rc` must be the live pointers the detoured `Draw` received, and `draw` must invoke the
/// block's original `Draw` trampoline.
unsafe fn nv_water_per_eye(
    this: *const NvWaterHighEndRenderBlock,
    rc: *mut RenderContext,
    mut draw: impl FnMut(),
) -> bool {
    if !Config::lock_query(|c| c.stereo.single_pass_nvwater_per_eye) {
        return false;
    }
    // SAFETY: `rc` is the live render context per the caller contract.
    let (pass, center_view) = unsafe { ((*rc).m_ActiveRenderPass, (*rc).m_View) };
    if NV_WATER_AUXILIARY_PASSES.contains(&pass) {
        return false;
    }
    let Some(eyes) = eye_cameras(center_view) else {
        return false;
    };
    // SAFETY: as above.
    let saved_projection = unsafe { (*rc).m_ProjectionF };

    let handled = crate::stereo::single_pass::draw_per_eye_half_ignoring_bound_vs(|eye| {
        // SAFETY: `rc` is live, and `this` is the live block whose `Setup` reads it. The two trailing
        // arguments are the draw-list sort ids, which this block's `Setup` override does not read.
        unsafe {
            (*rc).m_View = eyes[eye].view;
            (*rc).m_ProjectionF = eyes[eye].projection;
            SUPPRESS_SIMULATION_STEP.store(eye != 0, Ordering::Relaxed);
            (*this).Setup(rc, 0, 0);
        }
        draw();
    });

    SUPPRESS_SIMULATION_STEP.store(false, Ordering::Relaxed);
    // SAFETY: as above.
    unsafe {
        (*rc).m_View = center_view;
        (*rc).m_ProjectionF = saved_projection;
    }
    handled
}

/// The passes whose `CNvWaterHighEndRenderBlock::Draw` body is not the water surface: the compute
/// foam sub-pass, the wake prepass, and the painted-foam prepass. They render into the block's own
/// simulation and foam targets from their own viewports rather than into the scene, so the eye split
/// does not apply to them and `Setup` stages no view matrix for them either.
const NV_WATER_AUXILIARY_PASSES: [i32; 3] = [
    RenderPassId::PRE_RP_WATER_CS_PRE as i32,
    RenderPassId::PRE_RP_WATER_WAKES_PRE as i32,
    RenderPassId::PRE_RP_WATER_FOAM_PRE as i32,
];

/// Raised for the duration of the second eye's re-issue; see [`wave_works_simulation_step`].
static SUPPRESS_SIMULATION_STEP: AtomicBool = AtomicBool::new(false);

/// One eye's substitute for the render context's camera matrices.
struct EyeCamera {
    view: Matrix4,
    projection: Matrix4,
}

/// The two eyes' view and projection matrices, derived from the collapse's centre view the same way
/// the `SetupRenderCamera` hook derives the double-draw path's per-eye camera: offset the centre
/// camera's world transform by the eye's world offset, apply its head-local orientation delta on the
/// right (about the now-offset eye position), and invert. `None` when no VR frame is in flight.
fn eye_cameras(center_view: Matrix4) -> Option<[EyeCamera; 2]> {
    let center_world = glam::Mat4::from(center_view).inverse();
    Some([eye_camera(center_world, 0)?, eye_camera(center_world, 1)?])
}

fn eye_camera(center_world: glam::Mat4, eye: usize) -> Option<EyeCamera> {
    let params = crate::vr::render_params(eye)?;
    let mut world = center_world;
    world.w_axis += params.world_offset.extend(0.0);
    let world = world * glam::Mat4::from_quat(params.orientation_delta);
    Some(EyeCamera {
        view: Matrix4::from(world.inverse()),
        // The reverse-Z form, which is what the render camera's `m_ProjectionF` holds by the time a
        // render context is filled from it, under either projection convention.
        projection: params.projection_reverse_z,
    })
}

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
