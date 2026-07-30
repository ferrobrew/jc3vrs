//! The `DrawClustered` detour and the run driver it brackets: it decides how many times the block is
//! issued (once, or once per eye), arms the tile-bounds override and the per-eye froxel split around
//! each run, and puts every piece of pinned state back afterwards.

use std::{cell::Cell, ffi::c_void};

use detours_macro::detour;
use jc3gi::graphics_engine::{
    graphics_engine::{HTexture_t, RenderContext},
    render_block::RenderBlockDeferredLighting,
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    hooks::graphics_engine::{
        clustered_lighting::{
            bounds::tile_bounds_from_projection,
            split::{
                SPLIT, SplitState, TileGrid, assignment_transform, decline_split_for_context,
                decline_warning, restore_viewport, unsplittable_context,
            },
        },
        reconstruction,
    },
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&DRAW_CLUSTERED_BINDER)
}

thread_local! {
    /// Set while the original `DrawClustered` is running, so the `SetFragmentProgramConstants`
    /// detour knows to intercept the cb1 tile-bounds upload.
    pub(super) static CLUSTERED_ACTIVE: Cell<bool> = const { Cell::new(false) };

    /// The pre-computed whole-grid off-axis cb1 values for the current `DrawClustered` call, or `None`
    /// when no VR frame is in flight (flatscreen, or the fix is disabled). Used whenever the per-eye
    /// split is not engaged, including when a split run demotes itself.
    pub(super) static OFF_AXIS_CB1: Cell<Option<[f32; 8]>> = const { Cell::new(None) };
}

#[detour(
    address = jc3gi::graphics_engine::render_block::RenderBlockDeferredLighting::DrawClustered_ADDRESS
)]
fn draw_clustered(
    this: *const RenderBlockDeferredLighting,
    rc: *mut RenderContext,
    a3: *mut c_void,
    a4: *mut HTexture_t,
) {
    let (fix_frustum, per_eye, clustered, light_view) = Config::lock_query(|c| {
        (
            c.stereo.fix_clustered_light_frustum,
            c.stereo.single_pass.reconstruct_per_eye && c.stereo.reconstruct_offaxis_inverse,
            c.stereo.single_pass.clustered_per_eye,
            c.stereo.single_pass.clustered_per_eye_light_view,
        )
    });
    let run = |eye: Option<usize>| {
        run_draw_clustered(
            RunRequest {
                fix_frustum,
                eye,
                light_view,
            },
            this,
            rc,
            a3,
            a4,
        )
    };

    // SAFETY: `rc` is the live render context for this dispatch; the caller (the engine's draw
    // dispatch) guarantees it is valid for the duration of `DrawClustered`.
    let ctx = unsafe { rc.as_ref() }.map(|rc| rc.m_Context);

    // The froxel split rides on the cb1 override, so it cannot outrun it -- and it stands down on a
    // graphics context whose resolve has already proved unmaskable, for the reason in
    // `decline_split_for_context`'s doc comment.
    let split_eye = |eye: usize| {
        let splittable = ctx.is_some_and(|ctx| !unsplittable_context(ctx as usize));
        (clustered && fix_frustum && splittable).then_some(eye)
    };

    // `demoted_engaged` is read only when `split_fullscreen_pass` reports `Demoted`, in which case
    // `draw` (below) ran exactly once and this is that one run's `engaged` -- whether the froxel
    // narrowing took for eye 0 before the resolve turned out unmaskable and cut the split to one run.
    let mut demoted_engaged = false;
    let outcome = reconstruction::split_fullscreen_pass(per_eye, ctx, |eye| {
        demoted_engaged = run(split_eye(eye));
    });

    match outcome {
        // Whether the froxel narrowing engaged is only of interest on the demoted path below; an
        // un-split run has no half to leave unfilled.
        reconstruction::SplitOutcome::NotTaken => {
            run(None);
        }
        reconstruction::SplitOutcome::Split => {}
        reconstruction::SplitOutcome::Demoted => {
            // Eye 0's run was not masked, so it drew the whole target exactly as the un-split pass
            // does; there is no second run to fill the grid's other half. Worse than a wasted split: if
            // the froxel narrowing engaged anyway, the grid now holds eye 0's half beside a cleared
            // right half.
            //
            // The mask refuses when the bound viewport is not the collapse's double-wide target --
            // which is to say this dispatch is not the collapsed scene pass at all, so a per-eye grid
            // is meaningless for it. The assignment split engaged anyway because its own precondition
            // is weaker: it narrows as soon as the bound target is the tile grid it sized for, which an
            // off-scene dispatch can satisfy. The two preconditions disagreeing is the whole defect, and
            // the grid is shared, so the damage lands on the main scene's forward-lit geometry rather
            // than here.
            //
            // Maskability is a property of the pass, not of the run (see `MaskArming::
            // OnReconstruction`), so one observation settles it for this context: record it and let
            // every later dispatch on the same context skip the split outright. That costs one
            // half-built grid per context, once, and leaves the main scene's own context -- which does
            // mask -- splitting exactly as before.
            //
            // Rebuilding the grid here instead would mean re-running `DrawClustered`, resolve included,
            // which is the double exposure `split_fullscreen_pass_policy` exists to prevent.
            if demoted_engaged && let Some(ctx) = ctx {
                decline_split_for_context(ctx as usize);
            }
        }
    }

    crate::debug::pipeline_probes::record_main_color_mean("post_resolve");
}

/// What one `run_draw_clustered` is asked to do.
#[derive(Clone, Copy)]
struct RunRequest {
    /// Whether to override the tile bounds at all.
    fix_frustum: bool,
    /// The eye whose half of the tile grid this run should assign lights into, or `None` to build the
    /// whole grid as the un-split pass does.
    eye: Option<usize>,
    /// Whether to also assign that eye's lights from that eye's world position.
    light_view: bool,
}

/// One `DrawClustered`, with the off-axis tile-bounds override armed around it when enabled, and the
/// per-eye froxel split armed when requested. Reports whether the split actually engaged.
fn run_draw_clustered(
    request: RunRequest,
    this: *const RenderBlockDeferredLighting,
    rc: *mut RenderContext,
    a3: *mut c_void,
    a4: *mut HTexture_t,
) -> bool {
    // When the fix is disabled, call through without setting the thread-local flag.
    if !request.fix_frustum {
        DRAW_CLUSTERED.get().unwrap().call(this, rc, a3, a4);
        return false;
    }

    // SAFETY: `rc` is the live render context for this dispatch; the caller (the engine's draw
    // dispatch) guarantees it is valid for the duration of `DrawClustered`.
    let rc_ref = unsafe { rc.as_ref() };
    let grid = rc_ref.map(TileGrid::of);
    let ctx = rc_ref.map(|rc| rc.m_Context as usize).unwrap_or(0);
    // Un-split runs keep reading the collapse's single dispatch index, which is always eye 0.
    let dispatch_eye = request.eye.unwrap_or_else(crate::stereo::draw_index);
    let params = crate::vr::render_params(dispatch_eye);

    let whole_grid_cb1 = grid.zip(params).map(|(grid, params)| {
        tile_bounds_from_projection(&params.projection_standard, grid.exact_x, grid.exact_y, 0)
    });

    let split = request
        .eye
        .zip(grid)
        .zip(params)
        .and_then(|((eye, grid), params)| {
            if !grid.splittable() {
                decline_warning(&grid);
                return None;
            }
            let half_x = grid.exact_x / 2.0;
            Some(SplitState {
                eye,
                ctx,
                grid,
                cb0: assignment_transform(&params.projection_reverse_z, &grid),
                cb1: tile_bounds_from_projection(
                    &params.projection_standard,
                    half_x,
                    grid.exact_y,
                    eye,
                ),
                light_view_offset: request.light_view.then_some(params.world_offset),
                binds: 0,
                viewport_pinned: false,
                saved_viewport: None,
                engaged: false,
                demoted: false,
            })
        });

    let armed = whole_grid_cb1.is_some();
    if armed {
        CLUSTERED_ACTIVE.set(true);
        OFF_AXIS_CB1.set(whole_grid_cb1);
        SPLIT.set(split);
    }
    // The scope puts every pinned or suppressed piece of state back on *every* path out of the call,
    // so a leaked eye-half viewport or clear suppression cannot corrupt the rest of the frame.
    let scope = ClusteredScope(armed);

    DRAW_CLUSTERED.get().unwrap().call(this, rc, a3, a4);

    let engaged = SPLIT
        .get()
        .is_some_and(|state| state.engaged && !state.demoted);
    drop(scope);
    engaged
}

/// Restores everything [`run_draw_clustered`] arms, on every path out of the bracketed call.
struct ClusteredScope(bool);

impl Drop for ClusteredScope {
    fn drop(&mut self) {
        if !self.0 {
            return;
        }
        CLUSTERED_ACTIVE.set(false);
        OFF_AXIS_CB1.set(None);
        if let Some(state) = SPLIT.replace(None)
            && state.viewport_pinned
            && let Some(saved) = state.saved_viewport
        {
            restore_viewport(saved);
        }
    }
}
