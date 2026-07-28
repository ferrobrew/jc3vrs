//! The clustered-lighting froxel tile-bounds fix for off-axis VR projections (issue #35), and the
//! per-eye froxel grid under the single-pass collapse.
//!
//! `CRenderBlockDeferredLighting::DrawClustered` reconstructs a symmetric frustum from the vertical
//! FOV and aspect ratio, then uploads 8 floats (2 vec4s) to fragment constant buffer 1 (cb1) as
//! per-tile frustum edge bounds. The formula `boundX(i) = horiz * (1 - 2*i/tileCountX)` is always
//! centered on the optical axis and cannot encode the off-axis shift that VR per-eye projections
//! introduce — so lights are assigned to the wrong 64-pixel tiles, producing blocky, screen-aligned
//! lighting artifacts in VR.
//!
//! The geometry proxy transform (cb0, uploaded earlier in `DrawClustered`) is built from
//! `RenderContext::m_ProjectionF`, which already carries the off-axis projection (written by the
//! camera hook before `SetupRenderCamera`). So cb0 is correct outside the collapse; only cb1 needs
//! overriding there.
//!
//! Because the cb1 upload and the light-proxy draws both happen inside `DrawClustered`, we cannot
//! re-upload after the original returns. Instead, a thread-local flag is set around the original
//! `DrawClustered` call, and a detour on `Graphics::SetFragmentProgramConstants` intercepts the cb1
//! upload (identified by `cb_index=1, start_offset=0, count=2`) and replaces the data with
//! off-axis-derived values computed from the per-eye projection matrix. The second
//! `SetFragmentProgramConstants` call in `DrawClustered` (tile grid dimensions, `count=1`) is not
//! intercepted.
//!
//! Both `DrawClustered` and its `SetFragmentProgramConstants` calls run on the render thread, so the
//! thread-local flag correctly scopes the interception.
//!
//! # The per-eye resolve under the single-pass collapse
//!
//! `DrawClustered` ends with the deferred resolve: a fullscreen triangle whose vertex shader builds a
//! view ray from the `ViewProjInv` the [`super::reconstruction`] detour substitutes, and whose pixel
//! shader samples the sun-shadow cascade over the world positions that ray reconstructs. Under the
//! collapse that one draw spans **both** eye halves of the double-wide target while the substituted
//! basis describes one eye's frustum, so neither half reconstructs correctly and the error rotates
//! with the camera -- the sun shadows slide across the screen instead of staying on the world.
//!
//! Under [`StereoConfig::single_pass_reconstruct_per_eye`](crate::config::StereoConfig), the block's
//! whole `Draw` is re-issued once per eye through [`reconstruction::split_fullscreen_pass`], which masks
//! each run to that eye's half and hands it that eye's basis. The re-issue is of the whole block, not of
//! the resolve alone, because the resolve is not separately reachable: the mask has to be armed after
//! the block's last render-setup bind, and the block's own `PerspectiveFovInverse` call is the only seam
//! that sits between that bind and the draw. The froxel split below needs to know when the resolve could
//! not be masked at all, because that is exactly when it must decline rather than leave the grid
//! half-built -- `split_fullscreen_pass` reports that as [`reconstruction::SplitOutcome::Demoted`].
//!
//! # The per-eye froxel grid
//!
//! The block's earlier light-assignment phase therefore runs twice as well, and by default both runs
//! build the *same* grid: one eye's projection paired with the **double-wide** tile count, so the grid
//! is twice as wide as the frustum it describes and every local light lands in the wrong tiles. That
//! is wrong for both eyes, and it is not confined to this block -- ~20 forward-lit render-block types
//! bind the grid through `CLightManager::SetupForwardLightingResources`, and foliage reads it a whole
//! frame early, in `RP_VEGETATION_OPAQUE`.
//!
//! [`StereoConfig::single_pass_clustered_per_eye`](crate::config::StereoConfig) makes the assignment
//! per-eye too. Per run (see `docs/engine/lighting-shadow-pipeline.md` section 4.1 for how the engine
//! builds the grid, and `docs/mod/single-pass-stereo.md` for the split):
//!
//! 1. the light-assignment target's viewport is narrowed to this eye's half of the *tile* grid, which
//!    is a different half-width from the framebuffer's, so the collapse's own viewport helpers cannot
//!    be reused;
//! 2. the geometry transform (cb0) is rebuilt from this eye's projection;
//! 3. the tile bounds (cb1) are made affine in the **absolute** tile index, since the pixel shader
//!    derives its frustum from `SV_Position`, which includes the viewport's `TopLeftX`;
//! 4. and the second run's `Graphics::Clear` is suppressed.
//!
//! The two halves then compose: the assignment phase blends per tile with a commutative equation, the
//! eyes' tile halves are disjoint, and the compaction phase is per-tile-local. So the grid ends the
//! frame valid in **both** halves, which is what the forward consumers (this frame and next) need --
//! and the whole reason the clear has to be suppressed rather than the assignment simply repeated.
//!
//! Everything the split touches is a constant-buffer upload or viewport/clear state the mod already
//! intercepts; nothing is written into engine memory. It declines itself, leaving the un-split
//! behaviour intact, whenever the eye seam would not fall on a whole tile column or the bound
//! assignment target is not the grid it sized for.

use std::{
    cell::Cell,
    collections::BTreeSet,
    ffi::c_void,
    sync::atomic::{AtomicBool, Ordering},
};

use detours_macro::detour;
use glam::{Mat4, Vec3};
use jc3gi::{
    graphics_engine::{
        graphics_engine::{HContext_t, HTexture_t, RenderContext},
        render_block::RenderBlockDeferredLighting,
    },
    types::math::Matrix4,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;
use windows::Win32::Graphics::Direct3D11::D3D11_VIEWPORT;

use super::reconstruction::{self, with_immediate_context};
use crate::config::Config;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&DRAW_CLUSTERED_BINDER)
        .with_static_binder(&SET_FRAGMENT_PROGRAM_CONSTANTS_BINDER)
        .with_static_binder(&SET_GEOMETRY_PROGRAM_CONSTANTS_BINDER)
}

/// Whether the `Graphics::Clear` about to be issued on this thread is the light-assignment phase's
/// whole-target clear of a **second** eye's run, which must not wipe the first eye's half of the grid.
///
/// Called from the shared `Graphics::Clear` detour in [`crate::hooks::draw_count`] -- the engine issues
/// exactly one `Clear` inside `DrawClustered`, and the per-run scope confines this to it.
pub(crate) fn suppress_clear() -> bool {
    SPLIT
        .get()
        .is_some_and(|state| state.eye == 1 && state.viewport_pinned)
}

/// Note that a render setup has just been bound (and its viewport applied) on this thread, so a
/// per-eye froxel run can narrow the light-assignment viewport to its eye's half of the tile grid.
///
/// Called from the shared `Graphics::SetRenderSetup` detour in [`crate::hooks::draw_count`], *after*
/// the original has bound the setup's viewport. The first bind inside `DrawClustered` is the
/// light-assignment target's; the later ones (the compaction target, then the pass's own setup) rebind
/// the viewport themselves, which is what puts the narrowing back.
pub(crate) fn on_render_setup_bound() {
    let Some(mut state) = SPLIT.get() else {
        return;
    };
    let first = state.binds == 0;
    state.binds = state.binds.saturating_add(1);
    if first {
        match narrow_assignment_viewport(&state) {
            Some(saved) => {
                state.viewport_pinned = true;
                state.saved_viewport = Some(saved);
                state.engaged = true;
            }
            // Not the grid this run sized for: demote the whole run to the un-split path, which is
            // still safe to do here because the clear and both constant uploads come after this bind.
            None => state.demoted = true,
        }
    } else if state.viewport_pinned {
        state.viewport_pinned = false;
        state.saved_viewport = None;
    }
    SPLIT.set(Some(state));
}

/// The light-assignment vertex shader's `ViewMatrix` rows to upload in place of `data`, or `None` when
/// this upload is not that one or the per-eye light view is not engaged.
///
/// Called from the `Graphics::SetVertexProgramConstants` detour in [`crate::stereo::single_pass`],
/// which owns that entry point. `ctx` identifies which graphics context is staging the upload: the
/// detour sees every vertex-constant stage in the process, and `(cb_index, start_offset, count)` alone
/// does not identify this one -- `RenderBlockTerrainPatch` also stages exactly four rows at vertex
/// `cb1` offset 0, the same shape the sibling per-eye reprojection in `single_pass` guards against by
/// comparing `ctx` too. Without the check, a `RenderBlockTerrainPatch` stage that happens to land while
/// this split is active would have its rows overwritten with the light-assignment view instead.
pub(crate) fn substitute_assignment_view(
    ctx: usize,
    cb_index: i32,
    start_offset: u32,
    data: *const f32,
    count: u32,
) -> Option<[f32; MATRIX4_FLOATS]> {
    let state = active_split()?;
    if state.ctx != ctx {
        return None;
    }
    let offset = state.light_view_offset?;
    if cb_index != ASSIGNMENT_VIEW_CB
        || start_offset != 0
        || count != MATRIX4_ROWS
        || data.is_null()
    {
        return None;
    }
    let mut rows = [0.0f32; MATRIX4_FLOATS];
    // SAFETY: the caller stages `count` == 4 float4 rows from `data`, i.e. the 16 floats copied here.
    unsafe { std::ptr::copy_nonoverlapping(data, rows.as_mut_ptr(), MATRIX4_FLOATS) };
    // `RenderContext::m_View` is row-vector row-major (`view = p * M`), and the engine uploads its
    // rows 0..2 with a `(0, 0, 0, 1)` fourth row, i.e. the rotation with the translation dropped. The
    // proxy positions the CPU baked are relative to `m_RenderCameraPosition`, which under the collapse
    // is the centre head pose rather than either eye. Assigning from eye `e` means rotating
    // `p - offset` instead of `p`, and `(p - offset) * R == p * R - offset * R`, so the correction is
    // exactly a translation row of `-(offset * R)` -- the row the engine left empty.
    let row = |r: usize, c: usize| rows[r * MATRIX4_COLUMNS + c];
    let translation =
        [0, 1, 2].map(|c| -(offset.x * row(0, c) + offset.y * row(1, c) + offset.z * row(2, c)));
    let translation_row = 3 * MATRIX4_COLUMNS;
    rows[translation_row..translation_row + 3].copy_from_slice(&translation);
    Some(rows)
}

/// The engine's froxel tile size in pixels: `DrawClustered` derives the grid dimensions as
/// `m_DisplayWidth / 64` and `m_DisplayHeight / 64`, and the forward-lit shaders index the grid as
/// `ftoi(SV_Position.xy) >> 6`.
const TILE_SIZE: u32 = 64;

/// A `CMatrix4f` as the engine stages it: 4 float4 rows, row-major.
const MATRIX4_ROWS: u32 = 4;
const MATRIX4_COLUMNS: usize = 4;
const MATRIX4_FLOATS: usize = 16;

/// The geometry constant-buffer slot and row count of the light-assignment geometry shader's
/// `ProjMatrix`, uploaded as `SetGeometryProgramConstants(ctx, 0, 0, M, 4)`.
const ASSIGNMENT_TRANSFORM_CB: i32 = 0;

/// The vertex constant-buffer slot of the light-assignment vertex shader's `ViewMatrix`, uploaded as
/// `SetVertexProgramConstants(ctx, 2, 0, rows, 4)`.
const ASSIGNMENT_VIEW_CB: i32 = 2;

/// The fragment constant-buffer slot and row count of the per-tile frustum bounds, uploaded as
/// `SetFragmentProgramConstants(ctx, 1, 0, bounds, 2)`. The block's other `cb1` upload (the
/// light-chunk counts, in the compaction phase) has `count == 1`, so the row count discriminates them.
const TILE_BOUNDS_CB: i32 = 1;
const TILE_BOUNDS_ROWS: u32 = 2;

thread_local! {
    /// Set while the original `DrawClustered` is running, so the `SetFragmentProgramConstants`
    /// detour knows to intercept the cb1 tile-bounds upload.
    static CLUSTERED_ACTIVE: Cell<bool> = const { Cell::new(false) };

    /// The pre-computed whole-grid off-axis cb1 values for the current `DrawClustered` call, or `None`
    /// when no VR frame is in flight (flatscreen, or the fix is disabled). Used whenever the per-eye
    /// split is not engaged, including when a split run demotes itself.
    static OFF_AXIS_CB1: Cell<Option<[f32; 8]>> = const { Cell::new(None) };

    /// The per-eye froxel split in flight on this thread, or `None` outside one. Thread-local because
    /// the re-issue and everything it brackets run on the render thread, and the shared `Clear` /
    /// `SetRenderSetup` / constant-upload detours fire on other threads too.
    static SPLIT: Cell<Option<SplitState>> = const { Cell::new(None) };
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

/// The state a per-eye froxel run publishes for the detours that fire inside it.
#[derive(Clone, Copy)]
struct SplitState {
    /// The eye this run assigns lights for, and whose half of the tile grid it writes.
    eye: usize,
    /// The graphics context `DrawClustered` is running on, as the caller's raw pointer value. Keys
    /// [`substitute_assignment_view`]'s match so an unrelated vertex-constant stage on another context
    /// -- or another block's on this one -- cannot be mistaken for the light-assignment view upload.
    ctx: usize,
    grid: TileGrid,
    /// This eye's light-assignment geometry transform, substituted on `cb0`.
    cb0: [f32; MATRIX4_FLOATS],
    /// This eye's per-tile frustum bounds, affine in the absolute tile index, substituted on `cb1`.
    cb1: [f32; 8],
    /// This eye's world offset from the collapsed camera, when the per-eye light view is on.
    light_view_offset: Option<Vec3>,
    /// Render-setup binds seen so far in this run; the first is the light-assignment target's.
    binds: u32,
    /// Whether the assignment viewport is narrowed to this eye's half *right now* -- true only
    /// between the assignment target's bind and the next one, which is the window the clear falls in.
    viewport_pinned: bool,
    /// The viewport found bound before the narrowing, put back if the run ends while still narrowed.
    saved_viewport: Option<D3D11_VIEWPORT>,
    /// Whether the narrowing ever succeeded in this run.
    engaged: bool,
    /// Whether the narrowing was refused, demoting this run to the whole-grid path.
    demoted: bool,
}

/// The froxel tile grid `DrawClustered` builds for a dispatch.
#[derive(Clone, Copy)]
struct TileGrid {
    /// The fractional tile counts the engine quantises the bounds and the NDC nudge over.
    exact_x: f32,
    exact_y: f32,
    /// The grid's texel dimensions, `ceil` of the above -- and so the viewport the light-assignment
    /// render setup binds.
    texels_x: u32,
    texels_y: u32,
    /// The render width in pixels, which the eye seam bisects.
    width: u32,
    /// Whether the light-assignment phase runs at all this dispatch. With no lights the block skips
    /// it -- including its render-setup bind -- so the first bind we would see is the compaction
    /// target's, which is the same size and must not be narrowed.
    has_lights: bool,
}

impl TileGrid {
    fn of(rc: &RenderContext) -> Self {
        let width = rc.m_DisplayWidth.max(0) as u32;
        let height = rc.m_DisplayHeight.max(0) as u32;
        Self {
            exact_x: width as f32 / TILE_SIZE as f32,
            exact_y: height as f32 / TILE_SIZE as f32,
            texels_x: width.div_ceil(TILE_SIZE),
            texels_y: height.div_ceil(TILE_SIZE),
            width,
            has_lights: rc.m_ActivePointLightCount > 0 || rc.m_ActiveSpotLightCount > 0,
        }
    }

    /// Whether the eye seam falls on a whole tile column, so the two eyes' tiles are disjoint and the
    /// grid halves compose. A double-wide width that is not a multiple of `2 * TILE_SIZE` puts a
    /// partial tile column at the seam, which one eye would have to share with the other; there is no
    /// correct split of it, so the run declines rather than producing a wrong one.
    fn splittable(&self) -> bool {
        self.has_lights && self.width > 0 && self.width.is_multiple_of(2 * TILE_SIZE)
    }
}

/// The per-eye froxel split's live state, or `None` when it is not engaged for the run in flight.
fn active_split() -> Option<SplitState> {
    SPLIT.get().filter(|state| !state.demoted)
}

#[detour(address = jc3gi::graphics_engine::draw::SetFragmentProgramConstants_ADDRESS)]
fn set_fragment_program_constants(
    ctx: *mut HContext_t,
    cb_index: i32,
    start_offset: u32,
    data: *const f32,
    count: u32,
) {
    // Intercept the cb1 tile-bounds upload during DrawClustered's light-assignment pass. A per-eye
    // run substitutes bounds affine in the absolute tile index over its half of the grid; every other
    // run substitutes the whole-grid off-axis bounds.
    if CLUSTERED_ACTIVE.get()
        && cb_index == TILE_BOUNDS_CB
        && start_offset == 0
        && count == TILE_BOUNDS_ROWS
        && let Some(cb1) = active_split()
            .map(|state| state.cb1)
            .or_else(|| OFF_AXIS_CB1.get())
    {
        SET_FRAGMENT_PROGRAM_CONSTANTS.get().unwrap().call(
            ctx,
            cb_index,
            start_offset,
            cb1.as_ptr(),
            count,
        );
        return;
    }
    SET_FRAGMENT_PROGRAM_CONSTANTS
        .get()
        .unwrap()
        .call(ctx, cb_index, start_offset, data, count);
}

#[detour(address = jc3gi::graphics_engine::draw::SetGeometryProgramConstants_ADDRESS)]
fn set_geometry_program_constants(
    ctx: *mut HContext_t,
    cb_index: i32,
    start_offset: u32,
    data: *const f32,
    count: u32,
) {
    // The light-assignment geometry shader's `ProjMatrix`, built by the engine from the render
    // context's (single, collapsed) projection. A per-eye run substitutes its own eye's, which maps
    // that eye's NDC onto that eye's narrowed half of the tile grid by construction.
    if cb_index == ASSIGNMENT_TRANSFORM_CB
        && start_offset == 0
        && count == MATRIX4_ROWS
        && let Some(state) = active_split()
    {
        SET_GEOMETRY_PROGRAM_CONSTANTS.get().unwrap().call(
            ctx,
            cb_index,
            start_offset,
            state.cb0.as_ptr(),
            count,
        );
        return;
    }
    SET_GEOMETRY_PROGRAM_CONSTANTS
        .get()
        .unwrap()
        .call(ctx, cb_index, start_offset, data, count);
}

/// Narrow the currently-bound viewport to `state.eye`'s half of the tile grid, returning the viewport
/// it replaced. `None` -- leaving the device untouched -- when the immediate context is unreachable or
/// what is bound is not the tile grid this run sized for, in which case the run must not be split at
/// all.
fn narrow_assignment_viewport(state: &SplitState) -> Option<D3D11_VIEWPORT> {
    let full = with_immediate_context(|d3d| {
        let mut count = 1u32;
        let mut viewports = [D3D11_VIEWPORT::default(); 1];
        // SAFETY: `count` is the length of `viewports`, as `RSGetViewports` requires.
        unsafe { d3d.RSGetViewports(&mut count, Some(viewports.as_mut_ptr())) };
        viewports[0]
    })?;
    // One texel per 64-pixel tile: nothing else bound anywhere in the frame has these dimensions, so
    // this is what tells the light-assignment target apart from whatever else this run might see.
    if full.Width != state.grid.texels_x as f32 || full.Height != state.grid.texels_y as f32 {
        return None;
    }
    let half = full.Width / 2.0;
    let mut eye = full;
    eye.TopLeftX = full.TopLeftX + state.eye as f32 * half;
    eye.Width = half;
    // Both slots, and as a two-slot set: the single-pass viewport detour rewrites one-slot sets to
    // implement the eye split, and passes a two-slot set through untouched.
    set_viewports([eye, eye])?;
    if !SPLIT_LOGGED.swap(true, Ordering::Relaxed) {
        tracing::info!(
            target: "single_pass",
            "per-eye froxel grid engaged: the clustered light assignment now runs once per eye over \
             {half}x{} of the {}x{} tile grid, with the second run's clear suppressed",
            state.grid.texels_y,
            state.grid.texels_x,
            state.grid.texels_y,
        );
    }
    Some(full)
}

fn restore_viewport(saved: D3D11_VIEWPORT) {
    set_viewports([saved, saved]);
}

fn set_viewports(viewports: [D3D11_VIEWPORT; 2]) -> Option<()> {
    with_immediate_context(|d3d| {
        // SAFETY: a two-element slice is a valid viewport array.
        unsafe { d3d.RSSetViewports(Some(&viewports)) };
    })
}

/// Warn, once, that a per-eye froxel run declined because its grid cannot be halved on a tile column,
/// so the assignment ran whole-grid exactly as it does with the split off.
fn decline_warning(grid: &TileGrid) {
    if grid.has_lights && !DECLINE_LOGGED.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            target: "single_pass",
            "per-eye froxel grid declined: the {}px double-wide render width is not a multiple of \
             {}, so the eye seam falls inside a tile column and the two eyes' tiles would overlap; \
             the clustered light assignment ran whole-grid",
            grid.width,
            2 * TILE_SIZE,
        );
    }
}

/// Record that this graphics context's `DrawClustered` cannot be split, so later dispatches on it skip
/// the light-assignment split instead of half-building the shared grid again. Warns the first time.
///
/// Keyed on the context rather than latched globally: the scene's own context does mask, and letting an
/// off-scene dispatch stand the split down everywhere would trade a once-per-context artifact for
/// losing the per-eye grid for the whole session.
fn decline_split_for_context(ctx: usize) {
    if !UNSPLITTABLE_CONTEXTS.lock().insert(ctx) {
        return;
    }
    tracing::warn!(
        target: "single_pass",
        ctx = format!("{ctx:#x}"),
        "per-eye froxel light assignment declined for this graphics context: eye 0's run split the \
         assignment, but the resolve could not be masked to its half, which means the dispatch is not \
         drawing to the collapse's double-wide target and a per-eye grid does not apply to it. This \
         frame's grid is half-built; later dispatches on this context leave it whole.",
    );
}

/// Whether [`decline_split_for_context`] has stood the split down for `ctx`.
fn unsplittable_context(ctx: usize) -> bool {
    UNSPLITTABLE_CONTEXTS.lock().contains(&ctx)
}

static UNSPLITTABLE_CONTEXTS: Mutex<BTreeSet<usize>> = Mutex::new(BTreeSet::new());

/// One-shot latches for the coverage log lines above: the split either works for the whole session or
/// does not, so reporting it once is the whole signal.
static SPLIT_LOGGED: AtomicBool = AtomicBool::new(false);
static DECLINE_LOGGED: AtomicBool = AtomicBool::new(false);

/// The light-assignment geometry shader's `ProjMatrix`: the projection, plus the NDC nudge that snaps
/// the grid to whole tiles when the display size is not a multiple of [`TILE_SIZE`].
///
/// The engine builds `M = P · T(+1,-1,0) · Scaling(Tx/ceil(Tx), Ty/ceil(Ty), 1) · T(-1,+1,0)`, i.e. a
/// scale about the top-left NDC corner. Reproduced here so a per-eye run can substitute its own `P`.
/// The horizontal factor is 1 whenever the grid is splittable (a width that is a multiple of
/// `2 * TILE_SIZE` gives an integral tile count), and is written out rather than assumed so the
/// formula matches the engine's for any grid.
///
/// The engine's `CMatrix4f` is row-vector row-major and glam is column-vector column-major, so the
/// glam product is the reverse of the engine's, over the transposes the `Matrix4` bridge yields.
fn assignment_transform(projection: &Matrix4, grid: &TileGrid) -> [f32; MATRIX4_FLOATS] {
    let scale = Vec3::new(
        grid.exact_x / grid.texels_x.max(1) as f32,
        grid.exact_y / grid.texels_y.max(1) as f32,
        1.0,
    );
    let m = Mat4::from_translation(Vec3::new(-1.0, 1.0, 0.0))
        * Mat4::from_scale(scale)
        * Mat4::from_translation(Vec3::new(1.0, -1.0, 0.0))
        * Mat4::from(*projection);
    Matrix4::from(m).data
}

/// Compute the 8-float cb1 tile-bounds array from the off-axis projection matrix.
///
/// The symmetric formula in the original `DrawClustered` is:
///   horiz = tan(FOV/2) * aspect
///   vert = tan(FOV/2)
///   cb1[0] = -2 * horiz / tileCountX   (horizontal slope)
///   cb1[1] = horiz * (1 + 1/tileCountX) (horizontal max)
///   cb1[2] = horiz * (1 - 1/tileCountX) (horizontal min)
///   cb1[3] = 0
///   cb1[4..7] = same for vertical
///
/// For the off-axis case, replace `horiz` with the actual right bound and `2*horiz` (the full
/// extent) with `(right - left)`:
///   cb1[0] = -(right - left) / tileCountX
///   cb1[1] = right + (right - left) / (2 * tileCountX)
///   cb1[2] = right - (right - left) / (2 * tileCountX)
///   cb1[3] = 0
///   cb1[4] = -(top - bottom) / tileCountY
///   cb1[5] = top + (top - bottom) / (2 * tileCountY)
///   cb1[6] = top - (top - bottom) / (2 * tileCountY)
///   cb1[7] = 0
///
/// In the symmetric case, right = horiz and left = -horiz, so (right - left) = 2*horiz. The
/// off-axis formula generalizes this to arbitrary left/right bounds.
///
/// The frustum bounds are extracted from the projection matrix (row-major, row-vector):
///   right  = (1 + m[8]) / m[0]
///   left   = (m[8] - 1) / m[0]
///   top    = (1 + m[9]) / m[5]
///   bottom = (m[9] - 1) / m[5]
///
/// The reverse-Z remap (applied by `SetupRenderCamera` to `m_ProjectionF`) only changes column 2
/// (indices 2, 6, 10, 14), so m[0], m[5], m[8], m[9] are unaffected and the bounds can be extracted
/// from either the standard-depth or reverse-Z'd matrix.
///
/// `tile_count_x` is the count the horizontal bounds are quantised over -- the whole grid's, or one
/// eye's half of it under the per-eye split -- and `eye_column_offset` is how many multiples of that
/// count the pixel shader's absolute tile index runs ahead of the local one. The pixel shader derives
/// its frustum from `SV_Position`, which includes the viewport's `TopLeftX`, so shifting the local
/// index `j` to the absolute `i = j + eye * tile_count_x` is what makes the same 8 floats describe a
/// half of the grid that does not start at column 0. `eye_column_offset == 0` reduces to the
/// whole-grid form.
fn tile_bounds_from_projection(
    projection: &Matrix4,
    tile_count_x: f32,
    tile_count_y: f32,
    eye_column_offset: usize,
) -> [f32; 8] {
    let d = &projection.data;
    let right = (1.0 + d[8]) / d[0];
    let left = (d[8] - 1.0) / d[0];
    let top = (1.0 + d[9]) / d[5];
    let bottom = (d[9] - 1.0) / d[5];

    let h_extent = right - left;
    let v_extent = top - bottom;

    let h_half_tile = h_extent / (2.0 * tile_count_x);
    let v_half_tile = v_extent / (2.0 * tile_count_y);
    // Substituting `i -> i - eye * tile_count_x` into `bound(i) = right - h_extent * i / tile_count_x`
    // leaves the slope alone and shifts both biases by `eye * h_extent`.
    let h_origin = right + eye_column_offset as f32 * h_extent;

    [
        -h_extent / tile_count_x, // horizontal slope
        h_origin + h_half_tile,   // horizontal max
        h_origin - h_half_tile,   // horizontal min
        0.0,
        -v_extent / tile_count_y, // vertical slope
        top + v_half_tile,        // vertical max
        top - v_half_tile,        // vertical min
        0.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vr::projection::{Fov, OffAxisProjection};

    /// Extract the frustum bounds (right, left, top, bottom) from a row-major projection matrix.
    fn frustum_bounds(projection: &Matrix4) -> (f32, f32, f32, f32) {
        let d = &projection.data;
        let right = (1.0 + d[8]) / d[0];
        let left = (d[8] - 1.0) / d[0];
        let top = (1.0 + d[9]) / d[5];
        let bottom = (d[9] - 1.0) / d[5];
        (right, left, top, bottom)
    }

    /// Tile counts for a 1920×1080 display (the engine divides each dimension by 64).
    const TILE_COUNT_X: f32 = 1920.0 / TILE_SIZE as f32;
    const TILE_COUNT_Y: f32 = 1080.0 / TILE_SIZE as f32;

    fn asymmetric_fov() -> Fov {
        Fov {
            left: -30.0_f32.to_radians(),
            right: 50.0_f32.to_radians(),
            up: 35.0_f32.to_radians(),
            down: -45.0_f32.to_radians(),
        }
    }

    /// The off-axis tile-bounds computation must produce the same values as the original symmetric
    /// formula when given a symmetric projection matrix (left = -right, bottom = -top).
    #[test]
    fn test_off_axis_matches_symmetric_for_centered_frustum() {
        let half_fov_y = 50.0_f32.to_radians();
        let half_fov_x = 45.0_f32.to_radians();
        let fov = Fov {
            left: -half_fov_x,
            right: half_fov_x,
            up: half_fov_y,
            down: -half_fov_y,
        };
        let proj = OffAxisProjection::new(fov, 0.1, 38400.0).standard_depth;

        let cb1 = tile_bounds_from_projection(&proj, TILE_COUNT_X, TILE_COUNT_Y, 0);

        // The original symmetric formula from the decompile:
        //   v21 = tan(FOV/2)           // half vertical FOV
        //   v22 = v21 * aspect          // horiz = tan(FOV/2) * aspect
        //   v14 = 1 / tileCountX
        //   v15 = 1 / tileCountY
        //   cb1[0] = v14 * -2 * v22    = -2 * horiz / tileCountX
        //   cb1[1] = (v14 + 1) * v22   = (1/tileCountX + 1) * horiz
        //   cb1[2] = (1 - v14) * v22   = (1 - 1/tileCountX) * horiz
        //   cb1[3] = 0
        //   cb1[4] = v15 * -2 * v21    = -2 * vert / tileCountY
        //   cb1[5] = (v15 + 1) * v21   = (1/tileCountY + 1) * vert
        //   cb1[6] = (1 - v15) * v21   = (1 - 1/tileCountY) * vert
        //   cb1[7] = 0
        let vert = half_fov_y.tan();
        let aspect = half_fov_x.tan() / half_fov_y.tan();
        let horiz = vert * aspect;
        let inv_tx = 1.0 / TILE_COUNT_X;
        let inv_ty = 1.0 / TILE_COUNT_Y;

        let expected = [
            -2.0 * horiz * inv_tx,
            (inv_tx + 1.0) * horiz,
            (1.0 - inv_tx) * horiz,
            0.0,
            -2.0 * vert * inv_ty,
            (inv_ty + 1.0) * vert,
            (1.0 - inv_ty) * vert,
            0.0,
        ];

        for i in 0..8 {
            assert!(
                (cb1[i] - expected[i]).abs() < 1e-4,
                "cb1[{i}]: off-axis {} vs symmetric {}",
                cb1[i],
                expected[i]
            );
        }
    }

    /// The off-axis tile-bounds computation must produce asymmetric bounds (non-zero center shift)
    /// when given an asymmetric projection matrix.
    #[test]
    fn test_off_axis_produces_asymmetric_bounds() {
        let proj = OffAxisProjection::new(asymmetric_fov(), 0.1, 38400.0).standard_depth;

        let cb1 = tile_bounds_from_projection(&proj, TILE_COUNT_X, TILE_COUNT_Y, 0);

        // Recover the frustum centre from the uploaded constants, which is what makes this a
        // round-trip check rather than a restatement of the formula. `cb1[1]`/`cb1[2]` are
        // `right +/- half_tile`, so their mean is `right` -- *not* the centre. The extent comes from
        // the slope (`cb1[0] = -extent / tile_count`), and the centre is then `right - extent/2`,
        // which equals `(right + left) / 2`. It is non-zero iff the frustum is asymmetric.
        let h_extent = -cb1[0] * TILE_COUNT_X;
        let v_extent = -cb1[4] * TILE_COUNT_Y;
        let h_center = (cb1[1] + cb1[2]) / 2.0 - h_extent / 2.0;
        let v_center = (cb1[5] + cb1[6]) / 2.0 - v_extent / 2.0;

        assert!(
            h_center.abs() > 0.01,
            "horizontal center shift is {h_center}, expected non-zero"
        );
        assert!(
            v_center.abs() > 0.01,
            "vertical center shift is {v_center}, expected non-zero"
        );

        // Verify the center matches the projection's frustum center.
        let (right, left, top, bottom) = frustum_bounds(&proj);
        let expected_h_center = (right + left) / 2.0;
        let expected_v_center = (top + bottom) / 2.0;
        assert!(
            (h_center - expected_h_center).abs() < 1e-4,
            "horizontal center {h_center} vs expected {expected_h_center}"
        );
        assert!(
            (v_center - expected_v_center).abs() < 1e-4,
            "vertical center {v_center} vs expected {expected_v_center}"
        );
    }

    /// The frustum-bound extraction must match the known tangent values for a given FOV.
    #[test]
    fn test_frustum_bounds_from_projection() {
        let fov = Fov {
            left: -40.0_f32.to_radians(),
            right: 40.0_f32.to_radians(),
            up: 40.0_f32.to_radians(),
            down: -40.0_f32.to_radians(),
        };
        let proj = OffAxisProjection::new(fov, 0.1, 38400.0).standard_depth;

        let (right, left, top, bottom) = frustum_bounds(&proj);

        // For a symmetric frustum, right = tan(angleRight), left = tan(angleLeft), etc.
        assert!((right - fov.right.tan()).abs() < 1e-5, "right: {right}");
        assert!((left - fov.left.tan()).abs() < 1e-5, "left: {left}");
        assert!((top - fov.up.tan()).abs() < 1e-5, "top: {top}");
        assert!((bottom - fov.down.tan()).abs() < 1e-5, "bottom: {bottom}");
    }

    /// The per-eye bounds evaluated at the pixel shader's own sample point -- the **absolute** tile
    /// index plus a half -- must reproduce that eye's frustum edges over that eye's half of the grid.
    /// This is the property the whole split rests on: the same 8 floats have to describe a run of
    /// tiles that does not start at column 0.
    #[test]
    fn per_eye_tile_bounds_are_affine_in_the_absolute_tile_index() {
        let proj = OffAxisProjection::new(asymmetric_fov(), 0.1, 38400.0).standard_depth;
        let (right, left, top, bottom) = frustum_bounds(&proj);
        let half_tiles = TILE_COUNT_X / 2.0;

        for eye in 0..2 {
            let cb1 = tile_bounds_from_projection(&proj, half_tiles, TILE_COUNT_Y, eye);
            for j in 0..half_tiles as usize {
                // The pixel shader evaluates `v0.x * slope + bias` at the absolute tile index + 0.5.
                let absolute = eye as f32 * half_tiles + j as f32 + 0.5;
                let (max, min) = (absolute * cb1[0] + cb1[1], absolute * cb1[0] + cb1[2]);
                // Tile `j` of this eye spans `[right - extent*(j+1)/T, right - extent*j/T]`.
                let extent = right - left;
                let expected_max = right - extent * j as f32 / half_tiles;
                let expected_min = right - extent * (j + 1) as f32 / half_tiles;
                assert!(
                    (max - expected_max).abs() < 1e-4 && (min - expected_min).abs() < 1e-4,
                    "eye {eye} tile {j}: [{min}, {max}] vs expected [{expected_min}, {expected_max}]",
                );
            }
            // The vertical row does not halve: it must still span the full frustum height.
            let v_extent = -cb1[4] * TILE_COUNT_Y;
            assert!(
                (v_extent - (top - bottom)).abs() < 1e-4,
                "eye {eye}: vertical extent {v_extent} vs expected {}",
                top - bottom,
            );
            assert!(
                (cb1[5] - (top + v_extent / (2.0 * TILE_COUNT_Y))).abs() < 1e-4,
                "eye {eye}: vertical max {} is not the top edge plus a half tile",
                cb1[5],
            );
        }
    }

    /// The two eyes' halves must tile the whole grid exactly once: eye 0's first column starts at its
    /// own right edge, eye 1's last column ends at its own left edge, and neither reaches into the
    /// other's absolute tile range.
    #[test]
    fn per_eye_tile_bounds_cover_each_half_exactly_once() {
        let proj = OffAxisProjection::new(asymmetric_fov(), 0.1, 38400.0).standard_depth;
        let (right, left, _, _) = frustum_bounds(&proj);
        let half_tiles = TILE_COUNT_X / 2.0;

        for eye in 0..2 {
            let cb1 = tile_bounds_from_projection(&proj, half_tiles, TILE_COUNT_Y, eye);
            let first = eye as f32 * half_tiles + 0.5;
            let last = eye as f32 * half_tiles + half_tiles - 0.5;
            assert!(
                (first * cb1[0] + cb1[1] - right).abs() < 1e-4,
                "eye {eye}: first tile's max bound is not the frustum's right edge",
            );
            assert!(
                (last * cb1[0] + cb1[2] - left).abs() < 1e-4,
                "eye {eye}: last tile's min bound is not the frustum's left edge",
            );
        }
    }

    /// A grid whose double-wide width is not a multiple of two tiles has no correct split, and must be
    /// refused rather than seamed through a shared partial column.
    #[test]
    fn only_seam_aligned_grids_are_splittable() {
        let grid = |width: u32| TileGrid {
            exact_x: width as f32 / TILE_SIZE as f32,
            exact_y: 1080.0 / TILE_SIZE as f32,
            texels_x: width.div_ceil(TILE_SIZE),
            texels_y: 1080u32.div_ceil(TILE_SIZE),
            width,
            has_lights: true,
        };
        assert!(grid(2 * 1920).splittable(), "3840 = 30 * 128");
        assert!(grid(128).splittable());
        assert!(
            !grid(2 * 1900).splittable(),
            "3800 is not a multiple of 128"
        );
        assert!(!grid(192).splittable(), "1.5 tiles per eye");
        assert!(!grid(0).splittable());
        assert!(
            !TileGrid {
                has_lights: false,
                ..grid(2 * 1920)
            }
            .splittable(),
            "with no lights the assignment phase does not run at all",
        );
    }
}
