//! The remaining fullscreen depth-reconstruction passes, run once per eye under the single-pass
//! collapse: SSAO, screen-space reflections, and screen-space subsurface skin -- plus the
//! depth-of-field basis, which is the one consumer of the reconstruction that a per-eye split cannot
//! reach (see [`dof_get_view_proj_inverse`]).
//!
//! Cross-referencing `CMatrix4f::PerspectiveFovInverse` gives a closed set of seven consumers. The
//! deferred resolve ([`super::clustered_lighting`]) and the aerial perspective
//! ([`super::atmospheric_scattering`]) are split per eye in their own modules; the wireframe-only
//! `DrawPassThrough` never runs during shaded rendering; these are the rest. Collapsed, each of them
//! is one draw spanning **both** eye halves of the double-wide target while the substituted basis
//! describes one eye's frustum, so neither half reconstructs correctly and the error turns with the
//! camera.
//!
//! Each split is behind its own config flag and off by default, because unlike the two already landed
//! these blocks are not idempotent -- re-issuing the whole block re-runs work that accumulates:
//!
//! - **SSAO** advances its two-slot temporal history once per block draw (the inlined
//!   `SetNextHistoryBuffer`: `m_PrevFrameIndex = m_CurrFrameIndex; m_CurrFrameIndex ^= 1`, in the
//!   `m_EnableTemporalFilter` branch at the end of `CRenderBlockSSAO::Draw`), and clears
//!   `m_FirstPass`. That is handled: [`ssao_block_draw`] snapshots those three fields before the first
//!   eye and puts them back before the second, so both eyes resolve against the same history slot,
//!   write the same current slot, and the frame still advances the history exactly once.
//! - **SSR** re-runs its scene-colour capture, which is a *copy* rather than a consume (the block
//!   reads the scene colour and writes only its own targets), so the second run reproduces it.
//! - **Subsurface skin** builds its basis once per `Draw`, not once per blur axis: the two
//!   `PerspectiveFovInverse` calls in the function sit in mutually exclusive branches.
//!
//! What remains imperfect in all three -- and is the reason they stay default-off -- is that the mask
//! only arms at the block's own `PerspectiveFovInverse` call, so everything the block does *before*
//! that (SSAO's AO generation and its separable blur, SSR's capture and its compute blur) re-runs
//! unmasked across the whole target. Those blurs read and write the same textures, so a second run
//! blurs an already-blurred result. Compute dispatches ignore the scissor entirely.

use std::sync::atomic::{AtomicBool, Ordering};

use detours_macro::detour;
use jc3gi::{
    graphics_engine::{
        graphics_engine::{HContext_t, RenderContext},
        post_effects::PostEffectContext,
        render_block::{
            RBIInfo, RenderBlockSSAO, RenderBlockScreenSpaceReflection,
            RenderBlockScreenSpaceSubSurfaceSkin,
        },
        ssao::SSAOPass,
    },
    types::math::Matrix4,
};
use re_utilities::hook_library::HookLibrary;

use super::reconstruction;
use crate::config::Config;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&SSAO_BLOCK_DRAW_BINDER)
        .with_static_binder(&SSR_DRAW_BINDER)
        .with_static_binder(&SUBSURFACE_DRAW_BINDER)
        .with_static_binder(&DOF_GET_VIEW_PROJ_INVERSE_BINDER)
}

/// `CRenderBlockSSAO::Draw`, split per eye with its temporal history pinned across the two runs.
///
/// The history snapshot is taken inside the first run rather than around the whole split, so a run
/// that never masks (the split declining, e.g. because the bound viewport is not the double-wide
/// target) leaves the history exactly where the un-split pass would.
#[detour(address = jc3gi::graphics_engine::render_block::RenderBlockSSAO::Draw_ADDRESS)]
fn ssao_block_draw(this: *mut RenderBlockSSAO, rc: *mut RenderContext, info: *const RBIInfo) {
    let enabled = Config::lock_query(|c| {
        c.stereo.single_pass_ssao_per_eye && c.stereo.reconstruct_offaxis_inverse
    });
    let call = || SSAO_BLOCK_DRAW.get().unwrap().call(this, rc, info);

    let mut history = None;
    let mut first = true;
    split_or_issue(enabled, render_context_ctx(rc), || {
        if first {
            first = false;
            history = snapshot_ssao_history();
        } else if let Some(history) = history {
            restore_ssao_history(history);
        }
        call();
    });
}

/// `CRenderBlockScreenSpaceReflection::Draw`, split per eye.
///
/// The block's scene-colour capture is the first thing it does, before the basis is built, so it
/// re-runs unmasked -- but it is a copy out of the scene colour into the block's own target, and the
/// block writes nothing back into the scene colour, so the second run reproduces the same capture
/// rather than consuming it. The ray-march and the resolve that follow the basis are the masked part.
#[detour(
    address = jc3gi::graphics_engine::render_block::RenderBlockScreenSpaceReflection::Draw_ADDRESS
)]
fn ssr_draw(
    this: *mut RenderBlockScreenSpaceReflection,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let enabled = Config::lock_query(|c| {
        c.stereo.single_pass_ssr_per_eye && c.stereo.reconstruct_offaxis_inverse
    });
    let call = || SSR_DRAW.get().unwrap().call(this, rc, info);
    split_or_issue(enabled, render_context_ctx(rc), call);
}

/// `CRenderBlockScreenSpaceSubSurfaceSkin::Draw`, split per eye.
///
/// Both of the block's paths call `PerspectiveFovInverse` exactly once, before their first
/// render-setup bind, so the mask arms on the viewport left bound by the previous pass and covers
/// every draw of the diffusion chain.
#[detour(
    address = jc3gi::graphics_engine::render_block::RenderBlockScreenSpaceSubSurfaceSkin::Draw_ADDRESS
)]
fn subsurface_draw(
    this: *mut RenderBlockScreenSpaceSubSurfaceSkin,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let enabled = Config::lock_query(|c| {
        c.stereo.single_pass_subsurface_per_eye && c.stereo.reconstruct_offaxis_inverse
    });
    let call = || SUBSURFACE_DRAW.get().unwrap().call(this, rc, info);
    split_or_issue(enabled, render_context_ctx(rc), call);
}

/// `DOFUtil::GetViewProjInverse`, passed straight through -- the depth-of-field basis is the one
/// consumer of the reconstruction that this seam cannot fix, and the detour exists to say so out loud
/// when the flag is turned on.
///
/// Its only caller is `CDownScale2x2PackFocus::Apply`, the bokeh downscale prepass, which uploads the
/// matrix as vertex constants 1..4 of the fullscreen draw that ends the prepass. Two things make the
/// per-eye split unreachable there. The draw's target is the **quarter-resolution** packed DoF texture
/// (the prepass sizes its dispatches from `width / 4`), while
/// [`reconstruction::split_fullscreen_pass`] masks a half of the *double-wide scene* viewport and
/// declines any other target -- so a re-issue would run the prepass whole-target twice and split
/// nothing. And the prepass opens with five compute dispatches (the near-CoC pack plus four separable
/// blur passes ping-ponging two UAVs) which ignore the scissor and are not idempotent, so a second run
/// would blur the near field twice even if the mask did arm.
///
/// Substituting one eye's basis instead of re-issuing is not a fix either: that is exactly the defect
/// under the collapse, where one draw covers both halves. A correct fix needs a mask expressed in the
/// downscaled target's own space, which the shared seam does not offer.
#[detour(address = jc3gi::graphics_engine::post_effects::GetViewProjInverse_ADDRESS)]
fn dof_get_view_proj_inverse(out: *mut Matrix4, ctx: *mut PostEffectContext) -> *mut Matrix4 {
    if Config::lock_query(|c| c.stereo.single_pass_dof_per_eye)
        && crate::stereo::single_pass::collapse_active()
        && !DOF_WARNED.swap(true, Ordering::Relaxed)
    {
        tracing::warn!(
            target: "single_pass",
            "the per-eye depth-of-field flag has no effect: the bokeh downscale prepass draws into a \
             quarter-resolution packed target and opens with non-idempotent compute blur passes, so \
             neither the scissor mask nor a re-issue applies to it",
        );
    }
    DOF_GET_VIEW_PROJ_INVERSE.get().unwrap().call(out, ctx)
}

/// Run `draw` once per eye half under the collapse, falling back to issuing it exactly once.
///
/// [`reconstruction::split_fullscreen_pass`] returns `false` both when the split never started and
/// when it started and then demoted (the first run turned out not to mask, so it *was* the un-split
/// pass). Only the first case wants the caller's own issue; tracking whether the closure ran keeps the
/// second from drawing the pass twice.
fn split_or_issue(enabled: bool, ctx: Option<*mut HContext_t>, mut draw: impl FnMut()) {
    let mut ran = false;
    let split = reconstruction::split_fullscreen_pass(enabled, ctx, || {
        ran = true;
        draw();
    });
    if !split && !ran {
        draw();
    }
}

/// The graphics context a render block's dispatch is drawing on.
fn render_context_ctx(rc: *mut RenderContext) -> Option<*mut HContext_t> {
    // SAFETY: `rc` is the live render context for this dispatch; the engine's draw dispatch guarantees
    // it is valid for the duration of the block's `Draw`.
    unsafe { rc.as_ref() }.map(|rc| rc.m_Context)
}

/// The SSAO temporal-history state that `CRenderBlockSSAO::Draw` advances once per invocation.
#[derive(Clone, Copy)]
struct SsaoHistory {
    prev_frame_index: u32,
    curr_frame_index: u32,
    first_pass: bool,
}

/// The live pass's history state, or `None` before the first SSAO draw has recorded the pass.
fn snapshot_ssao_history() -> Option<SsaoHistory> {
    // SAFETY: the recorded pointer is the `CSSAOPass` the render block is drawing for -- the block's
    // `CRBIInfo` payload is the pass itself -- and the render thread owns it for the draw's duration.
    unsafe { ssao_pass().as_ref() }.map(|pass| SsaoHistory {
        prev_frame_index: pass.m_PrevFrameIndex,
        curr_frame_index: pass.m_CurrFrameIndex,
        first_pass: pass.m_FirstPass,
    })
}

fn restore_ssao_history(history: SsaoHistory) {
    // SAFETY: as `snapshot_ssao_history`.
    if let Some(pass) = unsafe { ssao_pass().as_mut() } {
        pass.m_PrevFrameIndex = history.prev_frame_index;
        pass.m_CurrFrameIndex = history.curr_frame_index;
        pass.m_FirstPass = history.first_pass;
    }
}

fn ssao_pass() -> *mut SSAOPass {
    super::ssao::ssao_pass()
}

/// One-shot latch for the depth-of-field line above: the answer is the same every frame.
static DOF_WARNED: AtomicBool = AtomicBool::new(false);
