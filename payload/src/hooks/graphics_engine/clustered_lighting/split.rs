//! The per-eye froxel split's state machine: the run state the detours around `DrawClustered` read,
//! the tile grid it sizes itself against, the light-assignment viewport narrowing and clear
//! suppression, the per-eye assignment view and geometry transform, and the bookkeeping that stands
//! the split down when a grid or a graphics context cannot carry it.

use std::{
    cell::Cell,
    collections::BTreeSet,
    sync::atomic::{AtomicBool, Ordering},
};

use glam::{Mat4, Vec3};
use jc3gi::{graphics_engine::graphics_engine::RenderContext, types::math::Matrix4};
use parking_lot::Mutex;
use windows::Win32::Graphics::Direct3D11::D3D11_VIEWPORT;

use crate::hooks::graphics_engine::{
    clustered_lighting::uploads::{
        ASSIGNMENT_VIEW_CB, MATRIX4_COLUMNS, MATRIX4_FLOATS, MATRIX4_ROWS,
    },
    reconstruction::with_immediate_context,
};

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
pub(super) const TILE_SIZE: u32 = 64;

/// The state a per-eye froxel run publishes for the detours that fire inside it.
#[derive(Clone, Copy)]
pub(super) struct SplitState {
    /// The eye this run assigns lights for, and whose half of the tile grid it writes.
    pub(super) eye: usize,
    /// The graphics context `DrawClustered` is running on, as the caller's raw pointer value. Keys
    /// [`substitute_assignment_view`]'s match so an unrelated vertex-constant stage on another context
    /// -- or another block's on this one -- cannot be mistaken for the light-assignment view upload.
    pub(super) ctx: usize,
    pub(super) grid: TileGrid,
    /// This eye's light-assignment geometry transform, substituted on `cb0`.
    pub(super) cb0: [f32; MATRIX4_FLOATS],
    /// This eye's per-tile frustum bounds, affine in the absolute tile index, substituted on `cb1`.
    pub(super) cb1: [f32; 8],
    /// This eye's world offset from the collapsed camera, when the per-eye light view is on.
    pub(super) light_view_offset: Option<Vec3>,
    /// Render-setup binds seen so far in this run; the first is the light-assignment target's.
    pub(super) binds: u32,
    /// Whether the assignment viewport is narrowed to this eye's half *right now* -- true only
    /// between the assignment target's bind and the next one, which is the window the clear falls in.
    pub(super) viewport_pinned: bool,
    /// The viewport found bound before the narrowing, put back if the run ends while still narrowed.
    pub(super) saved_viewport: Option<D3D11_VIEWPORT>,
    /// Whether the narrowing ever succeeded in this run.
    pub(super) engaged: bool,
    /// Whether the narrowing was refused, demoting this run to the whole-grid path.
    pub(super) demoted: bool,
}

/// The froxel tile grid `DrawClustered` builds for a dispatch.
#[derive(Clone, Copy)]
pub(super) struct TileGrid {
    /// The fractional tile counts the engine quantises the bounds and the NDC nudge over.
    pub(super) exact_x: f32,
    pub(super) exact_y: f32,
    /// The grid's texel dimensions, `ceil` of the above -- and so the viewport the light-assignment
    /// render setup binds.
    pub(super) texels_x: u32,
    pub(super) texels_y: u32,
    /// The render width in pixels, which the eye seam bisects.
    pub(super) width: u32,
    /// Whether the light-assignment phase runs at all this dispatch. With no lights the block skips
    /// it -- including its render-setup bind -- so the first bind we would see is the compaction
    /// target's, which is the same size and must not be narrowed.
    pub(super) has_lights: bool,
}

impl TileGrid {
    pub(super) fn of(rc: &RenderContext) -> Self {
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
    pub(super) fn splittable(&self) -> bool {
        self.has_lights && self.width > 0 && self.width.is_multiple_of(2 * TILE_SIZE)
    }
}

/// The per-eye froxel split's live state, or `None` when it is not engaged for the run in flight.
pub(super) fn active_split() -> Option<SplitState> {
    SPLIT.get().filter(|state| !state.demoted)
}

pub(super) fn restore_viewport(saved: D3D11_VIEWPORT) {
    set_viewports([saved, saved]);
}

/// Warn, once, that a per-eye froxel run declined because its grid cannot be halved on a tile column,
/// so the assignment ran whole-grid exactly as it does with the split off.
pub(super) fn decline_warning(grid: &TileGrid) {
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
pub(super) fn decline_split_for_context(ctx: usize) {
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
pub(super) fn unsplittable_context(ctx: usize) -> bool {
    UNSPLITTABLE_CONTEXTS.lock().contains(&ctx)
}

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
pub(super) fn assignment_transform(projection: &Matrix4, grid: &TileGrid) -> [f32; MATRIX4_FLOATS] {
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

thread_local! {
    /// The per-eye froxel split in flight on this thread, or `None` outside one. Thread-local because
    /// the re-issue and everything it brackets run on the render thread, and the shared `Clear` /
    /// `SetRenderSetup` / constant-upload detours fire on other threads too.
    pub(super) static SPLIT: Cell<Option<SplitState>> = const { Cell::new(None) };
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

fn set_viewports(viewports: [D3D11_VIEWPORT; 2]) -> Option<()> {
    with_immediate_context(|d3d| {
        // SAFETY: a two-element slice is a valid viewport array.
        unsafe { d3d.RSSetViewports(Some(&viewports)) };
    })
}

static UNSPLITTABLE_CONTEXTS: Mutex<BTreeSet<usize>> = Mutex::new(BTreeSet::new());

/// One-shot latches for the coverage log lines above: the split either works for the whole session or
/// does not, so reporting it once is the whole signal.
static SPLIT_LOGGED: AtomicBool = AtomicBool::new(false);
static DECLINE_LOGGED: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
mod tests {
    use super::*;

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
