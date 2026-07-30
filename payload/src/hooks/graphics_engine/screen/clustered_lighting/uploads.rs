//! The constant-buffer uploads inside `DrawClustered` that the mod substitutes: the fragment cb1 tile
//! bounds, and the light-assignment geometry shader's cb0 transform. Also holds the slot and matrix
//! layout facts every substitution matches an upload against, including the vertex-side one the
//! `SetVertexProgramConstants` owner calls into.

use detours_macro::detour;
use jc3gi::graphics_engine::graphics_engine::HContext_t;
use re_utilities::hook_library::HookLibrary;

use crate::hooks::graphics_engine::clustered_lighting::{
    draw::{CLUSTERED_ACTIVE, OFF_AXIS_CB1},
    split::active_split,
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&SET_FRAGMENT_PROGRAM_CONSTANTS_BINDER)
        .with_static_binder(&SET_GEOMETRY_PROGRAM_CONSTANTS_BINDER)
}

/// A `CMatrix4f` as the engine stages it: 4 float4 rows, row-major.
pub(super) const MATRIX4_ROWS: u32 = 4;
pub(super) const MATRIX4_COLUMNS: usize = 4;
pub(super) const MATRIX4_FLOATS: usize = 16;

/// The vertex constant-buffer slot of the light-assignment vertex shader's `ViewMatrix`, uploaded as
/// `SetVertexProgramConstants(ctx, 2, 0, rows, 4)`.
pub(super) const ASSIGNMENT_VIEW_CB: i32 = 2;

/// The geometry constant-buffer slot and row count of the light-assignment geometry shader's
/// `ProjMatrix`, uploaded as `SetGeometryProgramConstants(ctx, 0, 0, M, 4)`.
const ASSIGNMENT_TRANSFORM_CB: i32 = 0;

/// The fragment constant-buffer slot and row count of the per-tile frustum bounds, uploaded as
/// `SetFragmentProgramConstants(ctx, 1, 0, bounds, 2)`. The block's other `cb1` upload (the
/// light-chunk counts, in the compaction phase) has `count == 1`, so the row count discriminates them.
const TILE_BOUNDS_CB: i32 = 1;
const TILE_BOUNDS_ROWS: u32 = 2;

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
    //
    // The `state.ctx != ctx` guard mirrors `substitute_assignment_view`: without it, a cross-block
    // geometry-constant upload with the same `(cb_index, offset, count)` shape -- a terrain patch
    // stage that happens to land while the split is active -- would be substituted too.
    if cb_index == ASSIGNMENT_TRANSFORM_CB
        && start_offset == 0
        && count == MATRIX4_ROWS
        && let Some(state) = active_split()
        && state.ctx == ctx as usize
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
