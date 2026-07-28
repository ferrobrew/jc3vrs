//! The screen-space decal blocks' depth reconstruction, made per-eye under the single-pass collapse.
//!
//! A screen-space decal is a box volume rasterized over the G-buffer: its pixel shader samples the
//! scene depth under each covered pixel, reconstructs the camera-relative world position from it, and
//! keeps the pixel only if that position falls inside the decal's unit box. Two things it needs are
//! wrong under the collapse, and they are wrong in opposite directions:
//!
//! 1. **The reconstruction basis is pass state.** `CRenderBlockTypeSSDecal::Setup` builds
//!    `viewport-UV → NDC` composed with the inverse of the rotation-only view times the projection,
//!    and stages it on fragment `cb1[0..3]` **once per pass**. Under the collapse that projection is
//!    the single dispatch's -- one eye's -- so every decal in the other eye's half reconstructs
//!    against a frustum that is not its own, and the error turns with the camera.
//! 2. **The depth fetch addresses the whole double-wide target.** The same divided UV feeds both the
//!    basis rows and the depth-texture lookup. The basis wants the value normalized over *this eye's*
//!    viewport; the lookup wants it over the *double-wide* buffer, i.e. `(u + eye) · 0.5`. One value
//!    cannot be both, and the fetch is the one that is wrong: each eye reads the two-eye depth image
//!    stretched across its own half.
//!
//! So the fix is in two halves that only work together. This module re-issues the block's `Draw` once
//! per eye with that eye's half-viewport pinned, restages `cb1[0..3]` with that eye's basis, and
//! stages the horizontal offset `eye · 0.5` on a register past the ones the permutations use;
//! [`dxbc_stereo::bias_ssdecal_depth_uv`] rewrites those permutations to apply that offset to the
//! depth-fetch UV alone, leaving the basis rows reading the untouched per-viewport value. Neither half
//! is any use without the other: the rewrite with no staged offset biases nothing, and the staged
//! offset with no rewrite is read by nobody.
//!
//! The whole `Draw` is re-issued rather than just its `DrawIndexed`, because `Draw` also sets the
//! colour mask from the decal's channel flags and stages eight further fragment registers; re-issuing
//! the draw call alone would inherit whatever the last decal left behind. That is safe to do twice
//! because the pass blends with depth writes off and the two runs cover *disjoint* halves of the
//! target -- which is also why both viewport slots are pinned, so a permutation that routed to either
//! slot still lands in this eye.
//!
//! Behind [`StereoConfig::single_pass_ssdecal_per_eye`](crate::stereo::config::StereoConfig). The two halves
//! are decided at different points in the frame -- the rewrite when a shader is created, the staging
//! when a decal is drawn -- so the flag cannot switch them together: flipping it off leaves
//! already-rewritten permutations bound, fetching depth through an offset the draw path has stopped
//! staging. The debug UI therefore requests a shader reload when it changes, which is what actually
//! retracts (or applies) the rewrite, and [`shader_rewrite_enabled`] narrows the rewrite to the
//! configuration it can be right in. What is left is the interval between the toggle and the reload,
//! and [`neutralize_eye_bias`] holds the offset at a definite value across it.
//!
//! # The box geometry
//!
//! Those two together fix where each pixel of the decal reconstructs *from*. They do not move the box
//! itself: `Draw` bakes the instance's world-view-projection into **vertex** `cb1[0..3]` (followed by
//! the decal's view-independent box axes at `cb1[4..7]`), and the shared decal vertex program builds
//! clip with the multiply-add chain `Σ v.i · cb1[i] + cb1[3]`, so each of those four registers is a
//! *column* of the baked matrix and nothing in the collapse's per-eye machinery ever reaches them.
//! The decal therefore lands on the right surface in both eyes and at the same screen position in
//! both -- correct-looking in a screenshot, flat in a headset.
//!
//! Behind [`StereoConfig::single_pass_ssdecal_geometry_per_eye`](crate::stereo::config::StereoConfig), the
//! per-eye re-issue becomes
//! [`reproject_baked_cb_per_eye_staged`](crate::stereo::single_pass::reproject_baked_cb_per_eye_staged),
//! which post-multiplies that vertex upload by the eye's `M_eye` on its way through as the block
//! stages it. Only the vertex upload: the fragment restaging above is a separate constant buffer on a
//! separate stage and is left exactly as it is. The two are consistent afterwards rather than in
//! spite of each other -- the vertex program hands the pixel shader its screen UV as a projective
//! `TEXCOORD0` derived from the very clip position being reprojected, so moving the box into the
//! eye's own frustum is what makes the eye's own reconstruction basis the right one to read it with.

use std::sync::atomic::{AtomicBool, Ordering};

use detours_macro::detour;
use glam::Mat4;
use jc3gi::{
    graphics_engine::{
        draw::SetFragmentProgramConstants,
        graphics_engine::{HContext_t, RenderContext},
        render_block::{RBIInfo, RenderBlockSSDecal},
    },
    types::math::Matrix4,
};
use re_utilities::hook_library::HookLibrary;

use crate::config::Config;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&SS_DECAL_DRAW_BINDER)
}

/// Whether the decal permutations should be rewritten as they are created, so their depth fetch reads
/// the offset this module stages. Read by the `CreateFragmentProgram` hook, which owns every
/// fragment-shader rewrite.
///
/// Gated on the collapse as well as on the block's own flag, because the rewrite is only ever right
/// for a draw the per-eye re-issue below covers: the spliced instruction halves the depth-fetch UV
/// unconditionally, which addresses one eye's half of a *double-wide* buffer and nothing else. In any
/// other configuration -- the collapse off, or single-pass off entirely -- a rewritten permutation
/// would sample the left half of a normal-width depth buffer stretched across the decal, so leaving
/// those permutations pristine is what keeps decals correct there. Both halves of this gate are
/// sampled at shader-creation time, so a toggle only reaches the permutations at the next shader
/// reload; the debug UI requests one when the flag changes, and [`ss_decal_draw`] keeps the interval
/// in between bounded.
pub(super) fn shader_rewrite_enabled() -> bool {
    let enabled = crate::stereo::single_pass::collapse_active()
        && Config::lock_query(|c| c.stereo.single_pass.ssdecal_per_eye);
    if enabled {
        // Sticky, never cleared: this is the same gate `create_fragment_program` reads to decide
        // whether to splice the depth-UV offset into a permutation, so a `true` here means a
        // rewritten permutation may now exist in the pass, for the rest of the session, even past a
        // later toggle-off (the permutation itself stays bound, stale, until the reload it triggers
        // lands). [`neutralize_eye_bias`] reads this latch rather than re-deriving the condition,
        // because a live re-check would go back to `false` on toggle-off and skip the very interval
        // it exists to cover.
        EVER_REWRITTEN.store(true, Ordering::Relaxed);
    }
    enabled
}

/// Whether [`shader_rewrite_enabled`] has ever answered `true` this session, so a rewritten `ssdecal`
/// permutation may exist. See [`shader_rewrite_enabled`] for why this is a latch rather than a
/// re-derived condition.
static EVER_REWRITTEN: AtomicBool = AtomicBool::new(false);

#[detour(address = jc3gi::graphics_engine::render_block::RenderBlockSSDecal::Draw_ADDRESS)]
fn ss_decal_draw(this: *const RenderBlockSSDecal, rc: *mut RenderContext, info: *const RBIInfo) {
    let detour = SS_DECAL_DRAW.get().unwrap();
    // SAFETY: `rc` is the live render context the engine passed into `Draw`, read only for its
    // matrices and its graphics context; the closure re-invokes the original `Draw` trampoline.
    let handled = unsafe {
        per_eye(rc, || {
            detour.call(this, rc, info);
        })
    };
    if !handled {
        // This is the ordinary path -- single-pass off, the collapse off, or ordinary double-draw VR
        // all decline the per-eye re-issue and land here -- so the neutralizing write must not run
        // unconditionally: it stages into a slack row of a constant buffer pool shared with every
        // other block type in the frame (see `EYE_BIAS_REGISTER`), and it overwrites rather than
        // saves and restores whatever that row held. Gating on `EVER_REWRITTEN` confines that write
        // to sessions where a rewritten permutation could actually exist to be neutralized.
        if EVER_REWRITTEN.load(Ordering::Relaxed) {
            // SAFETY: `rc` is the live render context the engine passed into `Draw`.
            unsafe { neutralize_eye_bias(rc) };
        }
        detour.call(this, rc, info);
    }
}

/// Stage the left-half offset for a decal draw the per-eye re-issue declined, so a permutation that
/// *was* rewritten cannot read a stale value out of [`EYE_BIAS_REGISTER`].
///
/// The rewrite is decided at shader-creation time and the offset is staged at draw time, and the two
/// can disagree: the flag goes off (or the collapse does, or the VR frame the per-eye bases come from
/// is skipped) while permutations rewritten under the old answer are still bound. Those permutations
/// keep fetching depth through `cb1[13]`, and `cb1[13]` is a slack row of a pooled constant buffer
/// shared with every other block type in the frame, so with nothing staging it they read whatever the
/// last block happened to leave there -- an arbitrary offset, out of the texture as often as not, and
/// changing from draw to draw. Writing zero turns that into one definite, bounded outcome: the fetch
/// addresses the left half, which is what the pass did before this module existed. It cannot be made
/// *correct* here -- the spliced `· 0.5` has no neutral value -- so the shader reload the debug UI
/// requests on the toggle is still what retracts the rewrite; this only keeps the meantime stable.
///
/// A permutation that was never rewritten is unaffected: the row sits past the registers it declares
/// and no shader in the pass reads it.
///
/// # Safety
///
/// `rc` must be the live [`RenderContext`] the detoured `Draw` received.
unsafe fn neutralize_eye_bias(rc: *mut RenderContext) {
    // SAFETY: `rc` is live per the caller contract.
    let Some(rc_ref) = (unsafe { rc.as_ref() }) else {
        return;
    };
    let ctx = rc_ref.m_Context;
    if ctx.is_null() {
        return;
    }
    // SAFETY: `ctx` is the render context's live graphics context, and the offset is one float4 row.
    unsafe { stage(ctx, EYE_BIAS_REGISTER, &eye_bias(0), 1) };
}

/// The fragment constant-buffer slot the decal permutations read (`cbInstanceConsts`).
const INSTANCE_CB: i32 = 1;

/// The **vertex** constant-buffer slot the shared decal vertex program reads, also `cbInstanceConsts`
/// and also slot 1 -- a different buffer on a different stage from [`INSTANCE_CB`], staged through a
/// different entry point.
const VERTEX_INSTANCE_CB: i32 = 1;

/// The first of the four vertex registers [`RenderBlockSSDecal::Draw`] bakes the box's
/// world-view-projection into (`Draw` follows them with the box's axis basis at 4..7, which is
/// view-independent and must not be reprojected).
const WORLD_VIEW_PROJECTION_REGISTER: u32 = 0;

/// The first register of the pass's depth reconstruction basis, staged by the block type's `Setup`.
const BASIS_REGISTER: u32 = 0;

/// A `CMatrix4f` as the engine stages it: 4 float4 rows, row-major.
const MATRIX4_ROWS: u32 = 4;

/// Re-issue the decal `Draw` once per eye with that eye's reconstruction basis and depth-UV offset, or
/// return `false` when the intercept must not run (the flag is off, the render context or the per-eye
/// projections are unavailable, or the collapse intercept declines), in which case the caller draws
/// normally once.
///
/// # Safety
///
/// `rc` must be the live [`RenderContext`] the detoured `Draw` received, and `draw` must invoke the
/// block's original `Draw` trampoline.
unsafe fn per_eye(rc: *mut RenderContext, mut draw: impl FnMut()) -> bool {
    if !Config::lock_query(|c| c.stereo.single_pass.ssdecal_per_eye) {
        return false;
    }
    // SAFETY: `rc` is live per the caller contract.
    let Some(rc_ref) = (unsafe { rc.as_ref() }) else {
        return false;
    };
    let ctx = rc_ref.m_Context;
    if ctx.is_null() {
        return false;
    }
    let view = rotation_only(&rc_ref.m_View);
    let Some(bases) = eye_bases(&view) else {
        return false;
    };

    let mut render = |eye: usize| {
        // SAFETY: `ctx` is the render context's live graphics context; the basis is four float4 rows
        // and the offset is one.
        unsafe {
            stage(ctx, BASIS_REGISTER, &bases[eye], MATRIX4_ROWS);
            stage(ctx, EYE_BIAS_REGISTER, &eye_bias(eye), 1);
        }
        draw();
    };
    let handled = if Config::lock_query(|c| c.stereo.single_pass.ssdecal_geometry_per_eye) {
        // SAFETY: `rc` is the live render context the detoured `Draw` received, and `render` invokes
        // the block's original `Draw` trampoline.
        unsafe {
            crate::stereo::single_pass::reproject_baked_cb_per_eye_staged(
                rc,
                VERTEX_INSTANCE_CB,
                WORLD_VIEW_PROJECTION_REGISTER,
                crate::stereo::single_pass::BoundVsGate::Checked,
                &mut render,
            )
        }
    } else {
        crate::stereo::single_pass::draw_per_eye_half("SSDecal geometry", &mut render)
    };
    if handled {
        // Put the pass's own basis back, and the offset back to the left half. The type stages the
        // basis once per pass, ahead of every decal it covers, so leaving the second eye's behind
        // would hand it to any later decal this intercept declines -- and the staging array the
        // offset register lives in is shared with every other block type in the frame.
        // SAFETY: as above; `view` and `m_ProjectionF` are the matrices `Setup` itself composed.
        unsafe {
            stage(
                ctx,
                BASIS_REGISTER,
                &reconstruction_basis(&view, &rc_ref.m_ProjectionF),
                MATRIX4_ROWS,
            );
            stage(ctx, EYE_BIAS_REGISTER, &eye_bias(0), 1);
        }
    }
    handled
}

/// The register the rewritten permutations read their depth-UV offset from.
///
/// It is one past the thirteen registers
/// [`RenderBlockTypeSSDecal::SetupConstantBuffers`](jc3gi::graphics_engine::render_block::RenderBlockTypeSSDecal::SetupConstantBuffers)
/// declares for this pass, and staging it is still sound: the engine rounds a declared row count up
/// to the next pool size class before binding a buffer, so thirteen rows get a **sixteen**-row buffer
/// and the flush uploads all sixteen from the base row the slot was given (zero here). Registers 13,
/// 14, and 15 are that rounding's slack, none of them read by the pass's own permutations, and no
/// other slot can be reached through them -- every constant-buffer slot in the engine is declared with
/// base row zero, and in this pass fragment slots 2 and 3 are declared empty while slot 0 is a
/// directly bound globals buffer rather than a staged one. The slack rows do persist in the shared
/// staging array after the pass, which is why the restore below puts this one back.
const EYE_BIAS_REGISTER: u32 = dxbc_stereo::SSDECAL_EYE_BIAS_REGISTER;

/// The offset that maps a `[0, 1]` per-eye UV onto `eye`'s half of the double-wide depth texture,
/// paired with the `· 0.5` the rewritten instruction applies: `u' = u · 0.5 + eye · 0.5`.
fn eye_bias(eye: usize) -> [f32; 4] {
    [eye as f32 * 0.5, 0.0, 0.0, 0.0]
}

/// Stage `rows` float4 registers at `register` of the decal permutations' fragment constant buffer.
///
/// # Safety
///
/// `ctx` must be a live graphics context and `data` must hold at least `rows` float4 rows.
unsafe fn stage(ctx: *mut HContext_t, register: u32, data: &[f32], rows: u32) {
    unsafe { SetFragmentProgramConstants(ctx, INSTANCE_CB, register, data.as_ptr(), rows) };
}

/// Both eyes' reconstruction bases, or `None` when no VR frame is in flight (flatscreen, or a frame
/// the runtime asked to skip) and there is therefore no per-eye projection to build them from.
fn eye_bases(view: &Matrix4) -> Option<[[f32; 16]; 2]> {
    let params = [crate::vr::render_params(0)?, crate::vr::render_params(1)?];
    // `m_ProjectionF` carries the reverse-Z remap `SetupRenderCamera` applies, so the per-eye stand-in
    // for it is the reverse-Z off-axis matrix, not the standard-depth one.
    Some(params.map(|p| reconstruction_basis(view, &p.projection_reverse_z)))
}

/// The basis the block type's `Setup` composes: `UV_TO_NDC · inverse(view · projection)`, in the
/// engine's row-vector row-major storage.
///
/// The engine's `CMatrix4f` is row-vector row-major and glam is column-vector column-major, so the
/// `Matrix4` bridge yields transposes and the glam product runs in the reverse order: the engine's
/// `A · B` is glam's `Bᵀ · Aᵀ`, and `inverse(A)ᵀ` is `inverse(Aᵀ)`.
fn reconstruction_basis(view: &Matrix4, projection: &Matrix4) -> [f32; 16] {
    let clip_to_view = (Mat4::from(*projection) * Mat4::from(*view)).inverse();
    Matrix4::from(clip_to_view * Mat4::from_cols_array(&UV_TO_NDC)).data
}

/// The view matrix with its translation row replaced by `(0, 0, 0, 1)`, which is what `Setup` composes
/// with: the decal reconstructs a *camera-relative* position, so the view's translation is dropped.
fn rotation_only(view: &Matrix4) -> Matrix4 {
    let mut rows = view.data;
    rows[12..16].copy_from_slice(&[0.0, 0.0, 0.0, 1.0]);
    Matrix4 { data: rows }
}

/// The viewport-UV → NDC map `Setup` composes on the left: `Scaling(2, -2, 1)` with the translation
/// row `(-1, 1, 0, 1)`, so `(u, v)` in `[0, 1]` becomes `(2u - 1, 1 - 2v)`. Row-major, row-vector
/// convention (the translation is the last row).
#[rustfmt::skip]
const UV_TO_NDC: [f32; 16] = [
     2.0,  0.0, 0.0, 0.0,
     0.0, -2.0, 0.0, 0.0,
     0.0,  0.0, 1.0, 0.0,
    -1.0,  1.0, 0.0, 1.0,
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr::projection::{Fov, OffAxisProjection};

    fn asymmetric() -> Matrix4 {
        OffAxisProjection::new(
            Fov {
                left: -40.0_f32.to_radians(),
                right: 25.0_f32.to_radians(),
                up: 35.0_f32.to_radians(),
                down: -45.0_f32.to_radians(),
            },
            0.1,
            38400.0,
        )
        .reverse_z
    }

    /// A yawed, pitched, and translated view, to catch a basis that only happens to work at identity.
    fn view() -> Matrix4 {
        Matrix4::from(
            Mat4::from_rotation_y(0.7)
                * Mat4::from_rotation_x(-0.2)
                * Mat4::from_translation(glam::Vec3::new(120.0, -30.0, 4000.0)),
        )
    }

    /// The basis must be the exact inverse of the pipeline that produced the pixel: project a
    /// camera-relative point, take the viewport UV and depth the rasterizer would hand the shader, run
    /// them back through the basis, and land on the point again. That round trip is the whole contract
    /// of the reconstruction, and it is what a transposed factor or a reversed product would break.
    #[test]
    fn the_basis_inverts_the_projection_the_shader_sees() {
        let projection = asymmetric();
        let rotation = rotation_only(&view());
        let basis = Matrix4 {
            data: reconstruction_basis(&rotation, &projection),
        };
        // Row-vector `p · view · projection`, done in glam's reverse order over the bridged transposes.
        let world_to_clip = Mat4::from(projection) * Mat4::from(rotation);
        let to_basis = Mat4::from(basis);

        for point in [
            glam::Vec3::new(3.0, 1.0, 40.0),
            glam::Vec3::new(-25.0, 12.0, 300.0),
            glam::Vec3::new(60.0, -8.0, 1500.0),
        ] {
            let clip = world_to_clip * point.extend(1.0);
            let ndc = clip / clip.w;
            // What the rasterizer hands the pixel shader: the viewport UV and the depth-buffer value.
            let uv = glam::Vec2::new(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
            let reconstructed = to_basis * glam::Vec4::new(uv.x, uv.y, ndc.z, 1.0);
            let reconstructed = reconstructed.truncate() / reconstructed.w;
            assert!(
                (reconstructed - point).length() <= 1e-3 * point.length().max(1.0),
                "{point:?} reconstructed as {reconstructed:?}",
            );
        }
    }

    /// The two eyes must get *different* bases -- an off-axis pair whose shear is mirror-opposite
    /// cannot share one, and a bug that read the same eye twice would look exactly like the un-split
    /// pass while reporting that it had split.
    #[test]
    fn the_two_eyes_bases_differ_by_the_off_axis_shear() {
        let rotation = rotation_only(&view());
        let left = OffAxisProjection::new(
            Fov {
                left: -45.0_f32.to_radians(),
                right: 35.0_f32.to_radians(),
                up: 40.0_f32.to_radians(),
                down: -40.0_f32.to_radians(),
            },
            0.1,
            38400.0,
        )
        .reverse_z;
        let right = OffAxisProjection::new(
            Fov {
                left: -35.0_f32.to_radians(),
                right: 45.0_f32.to_radians(),
                up: 40.0_f32.to_radians(),
                down: -40.0_f32.to_radians(),
            },
            0.1,
            38400.0,
        )
        .reverse_z;

        let a = reconstruction_basis(&rotation, &left);
        let b = reconstruction_basis(&rotation, &right);
        assert!(
            a.iter().zip(b.iter()).any(|(x, y)| (x - y).abs() > 1e-4),
            "the two eyes' bases are identical: {a:?}",
        );
    }

    /// The staged offset and the `· 0.5` the rewritten instruction applies must together map a per-eye
    /// UV onto that eye's half of the double-wide depth texture, with the two halves disjoint and
    /// covering it exactly.
    #[test]
    fn the_eye_bias_maps_each_eye_onto_its_half() {
        for eye in 0..2 {
            let bias = eye_bias(eye)[0];
            for (u, expected) in [(0.0, 0.5 * eye as f32), (1.0, 0.5 * eye as f32 + 0.5)] {
                let biased = u * 0.5 + bias;
                assert!(
                    (biased - expected).abs() < 1e-6,
                    "eye {eye}: u {u} biased to {biased}, expected {expected}",
                );
            }
        }
    }

    /// The rotation-only view must keep the whole 3x3 basis and drop only the translation -- keeping it
    /// would reconstruct absolute world positions, which the decal box test is not expressed in.
    #[test]
    fn rotation_only_drops_the_translation_and_nothing_else() {
        let full = view();
        let stripped = rotation_only(&full);
        assert_eq!(stripped.data[..12], full.data[..12]);
        assert_eq!(stripped.data[12..], [0.0, 0.0, 0.0, 1.0]);
    }
}
