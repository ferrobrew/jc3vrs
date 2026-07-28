//! The remaining fullscreen depth-reconstruction passes, run once per eye under the single-pass
//! collapse: SSAO, screen-space reflections, screen-space subsurface skin, and the bokeh
//! depth-of-field prepass.
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
//! The rest of the non-idempotency is handled by *when* the mask goes up. These three run under
//! [`reconstruction::MaskArming::AtEntry`]: the eye mask is raised before the block draws anything and
//! re-derived from every render target the block binds, so the two runs are disjoint over the whole
//! block and each pixel is written exactly once. That is what the blur ping-pongs need -- SSAO's
//! bilateral blur reads and writes the same pair of occlusion textures, so two *overlapping* runs
//! would blur an already-blurred result -- and it is the only rule that can work at all here, because
//! each block draws through targets of several different sizes in one `Draw` while a scissor rectangle
//! means nothing outside the target it was measured in. (`CRenderBlockSSAO::Draw` alone touches the
//! full-resolution linear-depth targets, the half-resolution occlusion/history targets, and the
//! full-resolution scene target.)
//!
//! The compute work is not maskable at all -- a dispatch ignores the scissor as thoroughly as it
//! ignores the viewport -- and neither of the two blocks that dispatch offers any way to aim one: both
//! size their thread groups from the target's full width and address their textures straight off
//! `SV_DispatchThreadID`, with no origin term anywhere in their constant buffers. Nor can a UAV supply
//! one, since a D3D11 texture UAV selects a mip and an array slice and never a sub-rectangle. So the
//! compute work is *scheduled* rather than masked: [`reconstruction::DispatchPhase`] issues it
//! whole-target on exactly one run of the split -- the first for a prologue the masked draws consume
//! (the bokeh near-field coverage), the last for an epilogue that consumes what they produced (the SSR
//! blur). That is the same single whole-target pass the un-split block makes, so the eye-seam bleed
//! from the horizontal half of each separable blur (six texels of the block's own working resolution)
//! is inherent to the collapse rather than introduced here, and is present with these flags off.
//!
//! The ambient-occlusion block issues no dispatches and is fully covered by the mask, apart from its
//! mip generation, which regenerates the whole linear depth chain per run. That is idempotent, but the
//! first run builds it from a linear-depth target whose other half is still the previous frame's, so
//! the first run's occlusion draw reads stale coarse mips near the seam. The reach is bounded: the
//! chain is five levels, so a level-4 texel spans sixteen source texels and no texel outside sixteen of
//! the seam can see the stale half at all. That band is also the band in which a masked occlusion draw
//! is already wrong for a different reason -- a sample crossing the seam lands in the *other* eye's
//! view whatever the mips hold -- so the staleness adds no new class of error, and the split remains a
//! strict improvement on running the block once with one eye's basis.

use detours_macro::detour;
use jc3gi::graphics_engine::{
    graphics_engine::{HContext_t, RenderContext},
    post_effects::{DownScale2x2PackFocus, PostEffectContext, PostEffectsManager},
    render_block::{
        RBIInfo, RenderBlockSSAO, RenderBlockScreenSpaceReflection,
        RenderBlockScreenSpaceSubSurfaceSkin,
    },
    ssao::SSAOPass,
};
use re_utilities::hook_library::HookLibrary;

use super::reconstruction;
use crate::config::Config;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&SSAO_BLOCK_DRAW_BINDER)
        .with_static_binder(&SSR_DRAW_BINDER)
        .with_static_binder(&SUBSURFACE_DRAW_BINDER)
        .with_static_binder(&DOF_DOWNSCALE_APPLY_BINDER)
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

/// `CRenderBlockScreenSpaceReflection::Draw`, split per eye, with its compute blur left to the second
/// run.
///
/// The block's scene-colour capture is the first thing it does, before the basis is built; under the
/// at-entry mask each run captures only its own half, and in any case the capture is a copy out of the
/// scene colour into the block's own target -- the block writes nothing back into the scene colour --
/// so it is reproducible rather than consumed. The ray-march and the pixel-shader resolve that follow
/// are masked too.
///
/// The `m_UseComputeBlur` path cannot be masked, so it is *scheduled* instead:
/// [`reconstruction::DispatchPhase::LastRun`] suppresses its two dispatches on eye 0's run and lets
/// eye 1's issue them. The blur is the block's epilogue -- it consumes the ray-march's two targets and
/// writes the result back over them -- so by eye 1's run both halves have been ray-marched and one
/// whole-target blur is exactly the work the un-split block does. What that leaves is the horizontal
/// pass mixing across the eye seam, six of the block's texels either side of it; that is a property of
/// the collapse itself, present with this flag off and with the pixel-shader blur path too, and not
/// something the split introduces.
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
    split_or_issue_with(
        reconstruction::DispatchPhase::LastRun,
        enabled,
        render_context_ctx(rc),
        call,
    );
}

/// `CRenderBlockScreenSpaceSubSurfaceSkin::Draw`, split per eye.
///
/// Both of the block's paths call `PerspectiveFovInverse` exactly once, before their first
/// render-setup bind, and the at-entry mask covers every draw of the diffusion chain -- following the
/// six separable blur draws onto the SSS targets they ping-pong between, so neither run re-blurs the
/// other's output.
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

/// `CDownScale2x2PackFocus::Apply`, the bokeh downscale prepass, split per eye with its compute
/// prologue left to the first run.
///
/// This is the depth-of-field basis's home: the prepass's closing fullscreen draw is the sole consumer
/// of `DOFUtil::GetViewProjInverse`, which it uploads as vertex constants 1..4 and which the shared
/// `PerspectiveFovInverse` detour already substitutes the off-axis inverse into. Under the collapse
/// that one draw spans both halves with one eye's basis, so the haze the pack folds into the
/// downscaled colour is derived from the wrong world ray over most of the frame.
///
/// The prepass's five compute dispatches -- the near-field circle-of-confusion extract plus four
/// separable blur passes ping-ponging two quarter-resolution textures -- were the reason this was
/// previously reported as unsplittable, and they are still unmaskable. But they do not need a mask:
/// none of them reads a view or projection matrix (the extract takes the depth texture and the focal
/// tuning; the blur takes a step direction and its source texture), so their whole-target result is
/// already right for both eyes. [`reconstruction::DispatchPhase::FirstRun`] therefore issues them on
/// eye 0's run and suppresses them on eye 1's, which then reads the same blurred coverage instead of
/// blurring it a second time -- while both runs draw the pack masked to their own half with their own
/// basis. The blur's six-texel reach across the eye seam is left as it is: the un-split prepass has it
/// too, because the collapse hands the dispatch one double-wide texture either way.
#[detour(address = jc3gi::graphics_engine::post_effects::DownScale2x2PackFocus::Apply_ADDRESS)]
fn dof_downscale_apply(
    this: *mut DownScale2x2PackFocus,
    ctx: *mut HContext_t,
    pec: *mut PostEffectContext,
    mgr: *mut PostEffectsManager,
) {
    let enabled = Config::lock_query(|c| {
        c.stereo.single_pass_dof_per_eye && c.stereo.reconstruct_offaxis_inverse
    });
    let call = || DOF_DOWNSCALE_APPLY.get().unwrap().call(this, ctx, pec, mgr);
    split_or_issue_with(
        reconstruction::DispatchPhase::FirstRun,
        enabled,
        Some(ctx),
        call,
    );
}

/// [`split_or_issue_with`] for a block that issues no dispatches.
fn split_or_issue(enabled: bool, ctx: Option<*mut HContext_t>, draw: impl FnMut()) {
    split_or_issue_with(reconstruction::DispatchPhase::Both, enabled, ctx, draw);
}

/// Run `draw` once per eye half under the collapse, falling back to issuing it exactly once.
///
/// Every caller here goes through [`reconstruction::SplitPolicy::at_entry`], which is the split's only
/// constructor of [`MaskArming::AtEntry`](reconstruction::MaskArming::AtEntry) -- so this path can never
/// demote in the [`MaskArming::OnReconstruction`](reconstruction::MaskArming::OnReconstruction) sense of
/// a single unmasked run: under `AtEntry` the two runs are disjoint by construction, so the mask either
/// goes up before the first run draws anything -- in which case both runs happen -- or it never goes up
/// at all and nothing is drawn. [`reconstruction::SplitOutcome::NotTaken`] is therefore the only outcome
/// that means the caller's own issue is still needed; every other outcome means `draw` already ran and
/// must not run again.
fn split_or_issue_with(
    dispatches: reconstruction::DispatchPhase,
    enabled: bool,
    ctx: Option<*mut HContext_t>,
    mut draw: impl FnMut(),
) {
    let outcome = reconstruction::split_fullscreen_pass_policy(
        reconstruction::SplitPolicy::at_entry(dispatches),
        enabled,
        ctx,
        |_eye| draw(),
    );
    if outcome == reconstruction::SplitOutcome::NotTaken {
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
