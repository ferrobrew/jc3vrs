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
//! See [`StereoConfig::reconstruct_offaxis_inverse`](crate::stereo::config::StereoConfig).
//!
//! Substituting one eye's inverse presumes the pass is drawing *for* one eye, which the single-pass
//! collapse breaks: there the fullscreen quad covers both eye halves of the double-wide target in a
//! single draw, so one basis is right for neither half and the resulting error turns with the camera.
//! [`enter_per_eye_half_with`] is the seam for running such a pass once per eye instead, and
//! [`split_fullscreen_pass`] wraps it with the preconditions and demotion rule every consumer needs --
//! [`crate::hooks::graphics_engine::clustered_lighting`] drives it for the deferred resolve, which is where the sun shadow is
//! sampled.
//!
//! A per-eye half raises its mask under one of two [`MaskArming`] rules. The original one,
//! [`MaskArming::OnReconstruction`], waits for the pass's own `PerspectiveFovInverse` call, so a pass
//! that turns out not to reconstruct leaves the frame exactly as it found it; the price is that
//! whatever the block draws *before* that call runs unmasked, twice. The other,
//! [`MaskArming::AtEntry`], raises the mask before the block runs at all and follows it onto its own
//! render targets ([`on_render_setup_bound`]), so the two runs are disjoint over the whole block --
//! which is what a block whose early phases are non-idempotent (a separable blur ping-pong reading and
//! writing the same textures) needs, and what a block that draws through several differently-sized
//! targets needs, since a scissor rectangle is in the bound target's pixels and nothing else's.
//!
//! A mask cannot reach a block's *compute* work at all: a dispatch is not rasterization, so neither
//! the viewport nor the scissor rectangle restricts the texels its threads address
//! ([`Dispatch`](jc3gi::graphics_engine::draw::Dispatch)). A block that dispatches would therefore
//! redo its whole-texture compute work on the second run, over the first run's output. [`DispatchPhase`]
//! is the answer: it names *which single run* of the split issues the block's dispatches, leaving the
//! compute work done exactly once across the whole target -- which is what the un-split block does too,
//! so it introduces no error the collapse does not already have.

use std::{
    cell::Cell,
    sync::atomic::{AtomicBool, Ordering},
};

use detours_macro::detour;
use jc3gi::{
    graphics_engine::{draw::SetScissorEnable, graphics_engine::HContext_t},
    types::math::Matrix4,
};
use re_utilities::hook_library::HookLibrary;
use windows::Win32::{
    Foundation::RECT,
    Graphics::Direct3D11::{D3D11_RASTERIZER_DESC, D3D11_VIEWPORT, ID3D11DeviceContext},
};

use crate::{
    config::Config,
    debug::trace::{TraceEvent, TraceState},
    stereo::engine_context::EngineContext,
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
/// (an auxiliary camera, whose near/far are not the main view's) leaves the frame exactly as it found
/// it -- see [`PerEyeHalf::masked`].
///
/// Under [`MaskArming::AtEntry`] the mask goes up here, before the pass has drawn anything, and the
/// returned guard's [`PerEyeHalf::masked`] answers immediately: `false` means the mask could not be
/// raised at all (no readable viewport), so the caller has drawn nothing yet and can still fall back.
pub(super) fn enter_per_eye_half_with(
    arming: MaskArming,
    eye: usize,
    ctx: *mut HContext_t,
) -> PerEyeHalf {
    let saved = with_immediate_context(|d3d| {
        let mut count = 2u32;
        let mut rects = [RECT::default(); 2];
        // SAFETY: `count` is the length of `rects`, as `RSGetScissorRects` requires.
        unsafe { d3d.RSGetScissorRects(&mut count, Some(rects.as_mut_ptr())) };
        SavedScissor {
            rects,
            count: bound_scissor_count(&rects, count),
            enable: current_scissor_enable(d3d),
        }
    });
    let state = PerEyeState {
        eye,
        ctx,
        arming,
        masked: false,
        saved_scissor: saved,
    };
    PER_EYE.set(Some(state));
    if arming == MaskArming::AtEntry && !arm_eye_half_scissor(&state) {
        warn_entry_unmasked();
    }
    PerEyeHalf(())
}

/// When a per-eye half raises its scissor mask.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MaskArming {
    /// At the pass's own [`Matrix4::PerspectiveFovInverse`] call, which is the proof that the pass
    /// reconstructs at all: one that does not never gets masked and so is left byte-identical to its
    /// un-split self. Everything the pass draws before that call runs unmasked, on both runs.
    ///
    /// What that call is tested against is a property of the *pass* -- its near/far planes being the
    /// live main camera's, and the bound viewport being the collapse's double-wide one -- and not of
    /// the run or of anything the debug UI can move underneath it, so a split whose first run masks
    /// has every run mask. That is what makes the demotion in [`split_fullscreen_pass_policy`] a
    /// first-run property rather than something that can strike between the runs and leave them
    /// overlapping.
    OnReconstruction,
    /// Before the pass runs, and re-derived from every render target it binds thereafter
    /// ([`on_render_setup_bound`]), so the two runs are disjoint across the whole pass. The pass
    /// identity is the proof that it reconstructs; there is no observation to wait for, and no way to
    /// retract the mask once the first run's draws have been clipped by it.
    AtEntry,
}

/// Which single run of a two-run per-eye split issues the block's compute dispatches.
///
/// A scissor rectangle clips rasterization; it does nothing to a dispatch, whose reach is fixed by its
/// thread-group counts and by its program's mapping from thread id to texel. Both blocks this matters
/// for -- the screen-space reflection blur and the bokeh near-field prologue -- size their groups from
/// the target's full width and address their textures directly by `SV_DispatchThreadID` with no origin
/// term in any constant buffer, so there is no way to aim a dispatch at one eye's half. What there is
/// instead: run the compute work **once**, whole-target, on the one run of the split where the data it
/// needs is already there and the data it produces is still wanted.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum DispatchPhase {
    /// Both runs issue their dispatches, exactly as the un-split block does. The default, and the only
    /// correct choice for a block that issues none.
    Both,
    /// Only the first run. For a compute *prologue*, whose output the block's masked draws then
    /// consume: the first run computes it over the whole target, and the second run's masked draw reads
    /// the same result rather than recomputing it on top of itself.
    FirstRun,
    /// Only the last run. For a compute *epilogue*, which consumes what the block's masked draws
    /// produce: by the last run both halves have been drawn, so one whole-target pass over them is both
    /// complete and singular.
    LastRun,
}

/// The rule a [`split_fullscreen_pass_policy`] run follows: when its mask goes up, and which run issues
/// the block's dispatches.
#[derive(Clone, Copy)]
pub(super) struct SplitPolicy {
    arming: MaskArming,
    dispatches: DispatchPhase,
}

impl SplitPolicy {
    /// A split that masks at block entry and issues the block's dispatches on `dispatches`.
    ///
    /// Only [`MaskArming::AtEntry`] is offered: a [`DispatchPhase`] other than [`DispatchPhase::Both`]
    /// commits one run to *not* issuing the compute work before that run has drawn anything, and only
    /// an at-entry mask can promise that the run in question really is one half of a split rather than
    /// the whole pass in disguise.
    pub(super) fn at_entry(dispatches: DispatchPhase) -> Self {
        Self {
            arming: MaskArming::AtEntry,
            dispatches,
        }
    }
}

/// Whether the compute dispatch about to be issued on this thread belongs to a per-eye run that must
/// not issue it -- see [`DispatchPhase`].
///
/// Called from the shared `Graphics::Dispatch` / `Graphics::DispatchIndirect` detours in
/// [`crate::hooks::draw_count`]. Always `false` unless a per-eye run with a non-default
/// [`DispatchPhase`] is in flight on this thread, so every other dispatch in the engine is untouched.
pub(crate) fn dispatch_suppressed() -> bool {
    DISPATCH_GATE.get().is_some_and(|gate| match gate.phase {
        DispatchPhase::Both => false,
        DispatchPhase::FirstRun => gate.run != 0,
        DispatchPhase::LastRun => gate.run + 1 != gate.runs,
    })
}

/// Re-derive an [`MaskArming::AtEntry`] mask from the render target that was just bound on this
/// thread, whose viewport the bind has already applied.
///
/// Called from the shared `Graphics::SetRenderSetup` detour in [`crate::hooks::draw_count`]. A scissor
/// rectangle is expressed in the bound target's pixels, so a mask derived from the target the pass
/// started on stops meaning anything the moment the pass binds a target of a different size -- and the
/// blocks this covers do exactly that (the ambient-occlusion block draws through a full-resolution
/// linear-depth target, half-resolution occlusion and history targets, and the full-resolution scene
/// target, in one `Draw`). A no-op unless an at-entry per-eye half is in flight on this thread.
pub(crate) fn on_render_setup_bound() {
    let Some(state) = PER_EYE.get() else {
        return;
    };
    if state.arming != MaskArming::AtEntry || !state.masked {
        return;
    }
    if let Some(viewport) = current_viewport() {
        set_eye_half_scissor(viewport, state.eye);
    }
}

/// What a [`split_fullscreen_pass`] / [`split_fullscreen_pass_policy`] call did with `draw`.
///
/// The three outcomes are the three counts of how many times `draw` ran: zero, one, or two. That count
/// is what a caller actually needs to act correctly -- not "was the split taken" -- because a demoted
/// single run has already drawn the whole target and must not be issued again, exactly like a completed
/// split, while only a `NotTaken` precondition failure leaves the pass for the caller to issue itself.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SplitOutcome {
    /// The preconditions failed before anything ran -- no live collapse, no context, or no second eye
    /// to render -- so `draw` was never called. The caller must issue the pass itself, exactly once, as
    /// it always did.
    NotTaken,
    /// A run started and drew unmasked -- the whole target, exactly as the un-split pass would -- and
    /// the split stopped there rather than running a second eye that would draw the same full-width
    /// image a second time, dimming everything the pass accumulates. `draw` ran exactly once. This is
    /// sound for either run under [`MaskArming::OnReconstruction`] only because the arming condition is
    /// a property of the pass rather than of the run: a first run that masks means the second one masks
    /// too, so the whole-target case is the first run's.
    Demoted,
    /// Every run this call made was issued: ordinarily both eyes masked to their own half, or --
    /// reachable only under [`MaskArming::AtEntry`] -- the first eye masked and the second covered the
    /// rest whole-target because eye 0 had already drawn its half and there was no way back. `draw` ran
    /// twice.
    Split,
}

/// Run a fullscreen reconstruction pass once per eye half under the single-pass collapse, each run
/// masked to that eye and handed that eye's basis.
///
/// This is [`enter_per_eye_half_with`] plus the preconditions and the demotion rule that every consumer
/// needs, so a block's detour is the call and nothing else. `enabled` is the caller's own flag; the
/// shared preconditions (a live collapse, a context, and a second eye to render) are checked here. See
/// [`SplitOutcome`] for what the return value means to the caller.
pub(super) fn split_fullscreen_pass(
    enabled: bool,
    ctx: Option<*mut HContext_t>,
    draw: impl FnMut(usize),
) -> SplitOutcome {
    split_fullscreen_pass_policy(
        SplitPolicy {
            arming: MaskArming::OnReconstruction,
            dispatches: DispatchPhase::Both,
        },
        enabled,
        ctx,
        draw,
    )
}

/// [`split_fullscreen_pass`] generalized over [`SplitPolicy`], for a block that dispatches and whose
/// compute work must be issued on one run of the split only -- see [`DispatchPhase`].
///
/// `draw` is called once per run that actually executes, with that run's eye index, so a caller that
/// needs to act on what a particular run did -- e.g. record something only when [`SplitOutcome::Demoted`]
/// follows a run whose own work engaged -- can capture that out of `draw` itself into a variable the
/// caller reads back afterward, the same way [`crate::hooks::graphics_engine::fullscreen_reconstruction`]'s SSAO caller captures
/// its temporal-history snapshot across runs.
///
/// The gate covers every issue of `draw` this function makes, including the degenerate second-run one
/// below, and nothing outside them: a caller that falls back to issuing the pass itself does so
/// ungated, so the block dispatches exactly as it does with the split off.
pub(super) fn split_fullscreen_pass_policy(
    policy: SplitPolicy,
    enabled: bool,
    ctx: Option<*mut HContext_t>,
    mut draw: impl FnMut(usize),
) -> SplitOutcome {
    if !enabled || !crate::stereo::single_pass::collapse_active() {
        return SplitOutcome::NotTaken;
    }
    let Some(ctx) = ctx else {
        return SplitOutcome::NotTaken;
    };
    if crate::vr::render_params(1).is_none() {
        return SplitOutcome::NotTaken;
    }

    let mut draws = 0u32;
    for eye in 0..RUNS {
        let half = enter_per_eye_half_with(policy.arming, eye, ctx);
        if policy.arming == MaskArming::AtEntry && !half.masked() {
            // The mask never went up, so this run would cover the whole target. Before the first run
            // that is recoverable: nothing has been drawn, so report the split as not taken and let the
            // caller issue the pass once. Before the second it is not -- eye 0 drew only its half -- so
            // cover the rest the only way left, by running whole-target and accepting that eye 0's half
            // is drawn a second time. The dispatch gate stays on for it, because the compute work's
            // "once" is counted over the runs of the split and this is still the last of them.
            drop(half);
            if eye == 0 {
                return SplitOutcome::NotTaken;
            }
            let _gate = DispatchGate::enter(policy.dispatches, eye);
            draw(eye);
            draws += 1;
            return outcome_for(draws);
        }
        {
            let _gate = DispatchGate::enter(policy.dispatches, eye);
            draw(eye);
            draws += 1;
        }
        if !half.masked() {
            // Nothing was masked, so this run covered the whole target -- which is exactly what the
            // un-split pass does. Stop, and report it: `draw` has already run, so a caller that fell
            // back to issuing it itself would draw it a second time. `NotTaken` is reserved for the
            // precondition failures above, where `draw` has *not* run.
            return outcome_for(draws);
        }
    }
    outcome_for(draws)
}

/// [`SplitOutcome`] from a count of how many times `draw` ran -- see its doc comment for why the count
/// is the whole story.
fn outcome_for(draws: u32) -> SplitOutcome {
    match draws {
        0 => SplitOutcome::NotTaken,
        1 => SplitOutcome::Demoted,
        _ => SplitOutcome::Split,
    }
}

/// The scope opened by [`enter_per_eye_half_with`].
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
            // Restore the bit that was actually there, not an unconditional `false`: arming the mask
            // always turns scissor on, but the pass it interrupted may have had it on already for its own
            // reasons (a masked post pass, a clipped UI draw), and a bare disable would silently carry
            // that change into the rest of the frame instead of undoing only what this guard did.
            // `saved_scissor` is `None` only when the immediate context could not be reached at capture,
            // in which case nothing is known to restore and `false` is what this unconditionally did
            // before the bit was tracked.
            let enable = state.saved_scissor.is_some_and(|saved| saved.enable);
            // SAFETY: `ctx` is the graphics context the detoured `Draw` was running on, live for the
            // duration of the guard.
            unsafe { SetScissorEnable(state.ctx, enable) };
            if let Some(saved) = state.saved_scissor {
                with_immediate_context(|d3d| {
                    if saved.count > 0 {
                        // SAFETY: a `count`-length prefix of a two-element array is a valid rect slice of
                        // that length.
                        unsafe {
                            d3d.RSSetScissorRects(Some(&saved.rects[..saved.count as usize]))
                        };
                    }
                    // `count == 0` means neither slot was really bound -- writing the runtime's leftover
                    // (typically zeroed) rects back would install a clip-everything rect the moment
                    // anything downstream re-enables scissor.
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
    let (toggle, near_fallback, far_fallback) = Config::lock_query(|c| {
        (
            c.stereo.reconstruct_offaxis_inverse,
            c.vr.near_clip,
            c.vr.far_clip,
        )
    });
    // A per-eye half run reconstructs for *its* eye, not for the collapse's single dispatch index
    // (which is always eye 0); everything else keeps reading the dispatch's own eye.
    let half = PER_EYE.get();
    // Inside a per-eye half the toggle is not consulted: every caller that opens one sampled it once,
    // at block entry, and only opened the split because it was on, so the split's own runs must all
    // answer the way that sample did. Re-reading it here would let the debug UI flip it *between* the
    // two runs -- and this read is what arms the eye mask, so eye 0's run would mask to its half and
    // eye 1's would draw across the whole target, compositing eye 0's half twice into an accumulating
    // pass. Outside a split the live read stands: that is the plain detour path, with no second run to
    // disagree with.
    let enabled = toggle || half.is_some();
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
    // A per-eye half run masks the pass to this eye's half of the double-wide target and folds the
    // full-target-NDC -> eye-NDC remap into the inverse, so the half reconstructs exactly. If the mask
    // cannot be applied (the bound target is not the collapse's double-wide scene target), the run is
    // indistinguishable from the un-split pass, which is what [`PerEyeHalf::masked`] reports.
    //
    // The mask arms on `applies` -- the pass's identity -- and deliberately not on the per-eye
    // projections being available: `render_params` is read live too, and a run that armed while the
    // other did not would leave the two runs overlapping. A masked half that gets no substituted
    // inverse still draws exactly what the un-split whole-target pass would draw over those pixels
    // (the shaders derive their UV from their own NDC, which clipping does not move), so the split's
    // two runs tile the target exactly once whatever the live reads do mid-split.
    let masked_eye = match half {
        Some(state) if applies && arm_eye_half_scissor(&state) => Some(state.eye),
        _ => None,
    };
    let result = applies
        .then(|| params.map(|vr| glam::Mat4::from(vr.projection_reverse_z).inverse()))
        .flatten()
        .map(|inverse| match masked_eye {
            Some(eye) => Matrix4::from(inverse * half_target_remap(eye)),
            None => Matrix4::from(inverse),
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

/// The state [`enter_per_eye_half_with`] publishes for the reconstruction that runs inside it.
#[derive(Clone, Copy)]
struct PerEyeState {
    /// The eye this run renders.
    eye: usize,
    /// The graphics context the run's draws are issued on, whose rasterizer-state key carries the
    /// scissor-enable bit.
    ctx: *mut HContext_t,
    /// When this run raises its mask, and whether the mask follows the pass onto its own targets.
    arming: MaskArming,
    /// Whether the mask was actually applied; see [`PerEyeHalf::masked`].
    masked: bool,
    /// The scissor state found bound before the run, put back when the guard drops. `None` when the
    /// immediate context could not be reached, in which case nothing was changed either.
    saved_scissor: Option<SavedScissor>,
}

/// The scissor state [`enter_per_eye_half_with`] captures before it takes the rasterizer over, and
/// [`PerEyeHalf`]'s drop puts back.
#[derive(Clone, Copy)]
struct SavedScissor {
    /// The rectangles bound in slots 0 and 1, valid up to `count`.
    rects: [RECT; 2],
    /// How many leading slots of `rects` were actually bound; see [`bound_scissor_count`].
    count: u32,
    /// Whether scissor testing was enabled.
    enable: bool,
}

/// How many runs a per-eye split makes: one per eye half of the double-wide collapse target.
const RUNS: usize = 2;

/// Which run of a split is issuing draws right now, and under which [`DispatchPhase`]. Held only while
/// a run's `draw` closure is executing, so [`dispatch_suppressed`] answers for that run and nothing
/// else.
#[derive(Clone, Copy)]
struct DispatchGate {
    phase: DispatchPhase,
    run: usize,
    runs: usize,
}

impl DispatchGate {
    /// Open the gate for `run`, unless the phase is [`DispatchPhase::Both`] -- which suppresses nothing,
    /// so leaving the thread-local clear keeps [`dispatch_suppressed`] on the same branch it takes for
    /// every dispatch in the engine that has nothing to do with a split.
    ///
    /// Returns a guard that closes the gate on every path.
    fn enter(phase: DispatchPhase, run: usize) -> DispatchGateGuard {
        if phase != DispatchPhase::Both {
            DISPATCH_GATE.set(Some(Self {
                phase,
                run,
                runs: RUNS,
            }));
        }
        DispatchGateGuard(())
    }
}

/// The scope opened by [`DispatchGate::enter`].
struct DispatchGateGuard(());

impl Drop for DispatchGateGuard {
    fn drop(&mut self) {
        DISPATCH_GATE.set(None);
    }
}

thread_local! {
    /// The per-eye half in flight on this thread, or `None` outside one. Thread-local because the
    /// re-issue and the `PerspectiveFovInverse` call it brackets both run on the render thread, and a
    /// reconstruction on any other thread must not pick up the mask.
    static PER_EYE: Cell<Option<PerEyeState>> = const { Cell::new(None) };

    /// The [`DispatchGate`] in flight on this thread, or `None` outside one. Thread-local for the same
    /// reason [`PER_EYE`] is, and separate from it because the gate must also cover the one run that
    /// draws with the mask down.
    static DISPATCH_GATE: Cell<Option<DispatchGate>> = const { Cell::new(None) };
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

/// Clip the rest of this per-eye run to `state.eye`'s half of the bound target, by setting both
/// scissor rectangles to that half and raising the context's scissor-enable bit. Reports whether the
/// mask was applied; it is not when the immediate context is unreachable, nor -- under
/// [`MaskArming::OnReconstruction`] -- when the bound viewport is not the double-wide scene target
/// (a reduced-resolution or auxiliary pass, which must be left alone).
///
/// [`MaskArming::AtEntry`] keeps no such size condition. It arms before the pass has bound anything,
/// on whatever viewport the previous pass left, and every target the pass then binds re-derives the
/// rectangle through [`on_render_setup_bound`]; every one of those targets is sized from the collapse's
/// double-wide scene, so halving whichever is bound is the eye split at that target's resolution. This
/// mirrors what the collapse already does for viewports inside a per-eye re-issue, which likewise
/// halves the requested viewport rather than requiring a particular one.
///
/// Idempotent within a run: the second call finds the mask already up and reports success without
/// touching the device.
fn arm_eye_half_scissor(state: &PerEyeState) -> bool {
    if state.masked {
        return true;
    }
    let Some(full) = current_viewport() else {
        return false;
    };
    if state.arming == MaskArming::OnReconstruction {
        // The eye halves are derived from this viewport, so it has to be the collapse's full
        // double-wide one -- a pass that binds anything else is not the one we can split. A pixel of
        // slack for the engine's own rounding, matching the scene-size check the collapse's viewport
        // routing uses.
        let Some((width, _)) = crate::stereo::render_size() else {
            return false;
        };
        if (full.Width - width as f32).abs() > 1.0 {
            warn_unmasked(full.Width, width);
            return false;
        }
    }
    if set_eye_half_scissor(full, state.eye).is_none() {
        return false;
    }
    // `SetScissorEnable` is a pyxis-generated binding to the engine's own function, not a D3D11 API
    // method: it writes the context's rasterizer-state key field (`ctx->m_RasterizerStateKey`)
    // directly rather than issuing `RSSetScissorEnable`. The render thread owns the context at this
    // point -- the detour runs inside a `Draw` that the engine dispatched on it -- so there is no
    // race with an engine state flush. Routing it through `with_immediate_context` would add a
    // critical-section acquire on every per-draw call for no safety benefit.
    // SAFETY: `ctx` is the graphics context the bracketed pass draws on, live for the guard's scope.
    unsafe { SetScissorEnable(state.ctx, true) };
    PER_EYE.set(Some(PerEyeState {
        masked: true,
        ..*state
    }));
    if !ENGAGED.swap(true, Ordering::Relaxed) {
        let width = full.Width;
        tracing::info!(
            target: "single_pass",
            "per-eye reconstruction engaged: fullscreen reconstruction passes now run once per eye, \
             scissor-masked to each half of the {width}px-wide bound target",
        );
    }
    true
}

/// Set both scissor rectangles to `eye`'s half of `viewport`. `None` when the immediate context could
/// not be reached, in which case nothing was changed.
///
/// Both slots: a viewport-routed shader picks its scissor rectangle by the same index it picks its
/// viewport with, and the fullscreen quad's shader writes no index at all, so the two must agree.
fn set_eye_half_scissor(viewport: D3D11_VIEWPORT, eye: usize) -> Option<()> {
    let half = viewport.Width / 2.0;
    let left = viewport.TopLeftX + eye as f32 * half;
    let rect = RECT {
        left: left as i32,
        top: viewport.TopLeftY as i32,
        right: (left + half) as i32,
        bottom: (viewport.TopLeftY + viewport.Height) as i32,
    };
    with_immediate_context(|d3d| {
        // SAFETY: a two-element slice is a valid scissor-rect array.
        unsafe { d3d.RSSetScissorRects(Some(&[rect, rect])) };
    })
}

/// How many leading slots of a two-slot [`RSGetScissorRects`](ID3D11DeviceContext::RSGetScissorRects)
/// capture were really bound, independent of `reported` -- the count the runtime handed back.
///
/// Whether a runtime writes back the requested slot count or the number actually bound is
/// implementation-defined; [`capture_viewport_slots`](crate::stereo::single_pass) resolves the identical
/// ambiguity for viewport slots by trusting the extent instead of the count, since a slot nothing bound
/// reads back zero-width. A scissor rectangle has the same tell: a rect that is actually in effect always
/// has positive area, so a rect with none -- typically what an unset trailing slot reads back as -- is
/// the "not really there" signal, not `reported`. Restoring it once scissor is re-enabled later would
/// clip every subsequent primitive through that slot to nothing.
fn bound_scissor_count(rects: &[RECT; 2], reported: u32) -> u32 {
    rects
        .iter()
        .take(reported.min(2) as usize)
        .take_while(|r| r.right > r.left && r.bottom > r.top)
        .count() as u32
}

/// Whether scissor testing is enabled in the rasterizer state currently bound on the immediate context.
///
/// [`RSGetState`](ID3D11DeviceContext::RSGetState) reports no object at all (rather than a real one) only
/// when nothing has ever been explicitly bound, which is exactly the D3D11 default rasterizer state --
/// whose `ScissorEnable` is `FALSE` -- so that case folds into the same answer a bound state would give
/// if it matched the default.
fn current_scissor_enable(d3d: &ID3D11DeviceContext) -> bool {
    // SAFETY: `d3d` is the live immediate context.
    let Ok(state) = (unsafe { d3d.RSGetState() }) else {
        return false;
    };
    let mut desc = D3D11_RASTERIZER_DESC::default();
    // SAFETY: `desc` is a valid receiver for a rasterizer-state description.
    unsafe { state.GetDesc(&mut desc) };
    desc.ScissorEnable.as_bool()
}

/// The viewport bound on the immediate context, which is the space a scissor rectangle set now would
/// be interpreted in.
fn current_viewport() -> Option<D3D11_VIEWPORT> {
    with_immediate_context(|d3d| {
        let mut count = 1u32;
        let mut viewports = [D3D11_VIEWPORT::default(); 1];
        // SAFETY: `count` is the length of `viewports`, as `RSGetViewports` requires.
        unsafe { d3d.RSGetViewports(&mut count, Some(viewports.as_mut_ptr())) };
        viewports[0]
    })
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

/// Warn, once, that an at-entry per-eye run could not read a viewport to derive its mask from, so the
/// pass was left to run whole-target exactly as it does with the split off.
fn warn_entry_unmasked() {
    if !ENTRY_UNMASKED_WARNED.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            target: "single_pass",
            "per-eye reconstruction declined at block entry: no viewport could be read from the \
             immediate context, so the pass ran once across the whole target with one eye's basis",
        );
    }
}

/// One-shot latches for the three coverage log lines above: the split is either working for the whole
/// session or not, so reporting it once is the whole signal and a per-frame line would be noise.
static ENGAGED: AtomicBool = AtomicBool::new(false);
static UNMASKED_WARNED: AtomicBool = AtomicBool::new(false);
static ENTRY_UNMASKED_WARNED: AtomicBool = AtomicBool::new(false);

/// Resolve the engine's immediate D3D context and run `f` on it under the context mutex. `None` when
/// the device or context is not live yet.
///
/// A thin resolve-per-call wrapper over [`EngineContext`], kept because the callers here touch the
/// context a handful of times per frame and have nothing to gain from holding a handle -- unlike the
/// single-pass draw paths, which resolve once and carry it across a whole re-issue.
pub(super) fn with_immediate_context<R>(f: impl FnOnce(&ID3D11DeviceContext) -> R) -> Option<R> {
    Some(EngineContext::get()?.with_lock(f))
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
