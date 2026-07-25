//! Render-thread detours for single-pass stereo (experimental; see `docs/mod/single-pass-stereo.md`
//! and [`crate::stereo::single_pass`]).
//!
//! When single-pass is active, the patched vertex shaders read their position from the mod-owned
//! `cb13` instead of `cb0`. The cb13-sync detour keeps `cb13` in sync: after the engine refreshes and
//! uploads the global VS constants (`m_VPGlobalConstData` → `cb0`), it mirrors the current view's
//! per-eye rows into `cb13` and binds it at `b13`. It runs per pass (the same cadence as the engine's
//! own `cb0` upload), so `cb13` always matches whatever view is current -- the G-buffer eye view, but
//! also the shadow/reflection views that reuse the same model shaders.
//!
//! The render-block detours below cover the blocks that bake their view-projection into a constant
//! buffer inside their own `Draw` (bark, foliage, occluder), across draw kinds that can't be
//! instance-doubled. When the block's flag is on and the collapse is active, the block's `Draw` is
//! re-issued once per eye with that baked constant reprojected by the eye's `M_eye`
//! ([`reproject_baked_cb_per_eye`](crate::stereo::single_pass::reproject_baked_cb_per_eye)); otherwise
//! it draws once, unchanged. Each is gated by its own default-off config flag.

use std::ffi::c_void;

use detours_macro::detour;
use jc3gi::graphics_engine::{
    graphics_engine::RenderContext,
    render_block::{RBIInfo, RenderBlockBark, RenderBlockFoliage, RenderBlockOccluder},
    render_engine::RenderEngine,
};
use re_utilities::hook_library::HookLibrary;

use crate::stereo::single_pass::BlockIntercept;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&SET_ALL_GLOBAL_SHADER_PROGRAM_CONSTANTS_BINDER)
        .with_static_binder(&RENDER_BLOCK_BARK_DRAW_BINDER)
        .with_static_binder(&RENDER_BLOCK_BARK_DRAW_Z_BINDER)
        .with_static_binder(&RENDER_BLOCK_FOLIAGE_DRAW_BINDER)
        .with_static_binder(&RENDER_BLOCK_OCCLUDER_DRAW_Z_BINDER)
}

/// The tree-trunk/branch block bakes its world-view-projection into vertex `cb1` registers 0..3, so
/// reproject those four rows per eye. Covers all three of its draw kinds (plain, CPU-instanced,
/// GPU-indirect) via the whole-`Draw` re-issue.
#[detour(address = jc3gi::graphics_engine::render_block::RenderBlockBark::Draw_ADDRESS)]
fn render_block_bark_draw(
    this: *const RenderBlockBark,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let detour = RENDER_BLOCK_BARK_DRAW.get().unwrap();
    // SAFETY: `this`/`rc`/`info` are the live pointers the engine passed in; the closure re-invokes the
    // original `Draw` trampoline.
    let handled = crate::stereo::single_pass::block_intercept_enabled(BlockIntercept::Bark)
        && unsafe {
            crate::stereo::single_pass::reproject_baked_cb_per_eye(rc, 1, 0, || {
                detour.call(this, rc, info);
            })
        };
    if !handled {
        detour.call(this, rc, info);
    }
}

/// The tree-trunk/branch depth-and-velocity pass bakes the same `cb1` view-projection; reproject it too.
#[detour(address = jc3gi::graphics_engine::render_block::RenderBlockBark::DrawZ_ADDRESS)]
fn render_block_bark_draw_z(
    this: *const RenderBlockBark,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let detour = RENDER_BLOCK_BARK_DRAW_Z.get().unwrap();
    // SAFETY: as above.
    let handled = crate::stereo::single_pass::block_intercept_enabled(BlockIntercept::Bark)
        && unsafe {
            crate::stereo::single_pass::reproject_baked_cb_per_eye(rc, 1, 0, || {
                detour.call(this, rc, info);
            })
        };
    if !handled {
        detour.call(this, rc, info);
    }
}

/// The grass/foliage block bakes its view-projection into vertex `cb2` registers 4..7 (register 0 is
/// the per-draw world matrix); reproject the `cb2` view-projection rows per eye.
#[detour(address = jc3gi::graphics_engine::render_block::RenderBlockFoliage::Draw_ADDRESS)]
fn render_block_foliage_draw(
    this: *const RenderBlockFoliage,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let detour = RENDER_BLOCK_FOLIAGE_DRAW.get().unwrap();
    // SAFETY: as above.
    let handled = crate::stereo::single_pass::block_intercept_enabled(BlockIntercept::Foliage)
        && unsafe {
            crate::stereo::single_pass::reproject_baked_cb_per_eye(rc, 2, 4, || {
                detour.call(this, rc, info);
            })
        };
    if !handled {
        detour.call(this, rc, info);
    }
}

/// The occluder depth-prime block's non-instanced path bakes its world-view-projection into vertex
/// `cb1` registers 0..3; reproject it per eye so each eye's depth is primed with its own projection.
#[detour(address = jc3gi::graphics_engine::render_block::RenderBlockOccluder::DrawZ_ADDRESS)]
fn render_block_occluder_draw_z(
    this: *const RenderBlockOccluder,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let detour = RENDER_BLOCK_OCCLUDER_DRAW_Z.get().unwrap();
    // SAFETY: as above.
    let handled = crate::stereo::single_pass::block_intercept_enabled(BlockIntercept::Occluder)
        && unsafe {
            crate::stereo::single_pass::reproject_baked_cb_per_eye(rc, 1, 0, || {
                detour.call(this, rc, info);
            })
        };
    if !handled {
        detour.call(this, rc, info);
    }
}

#[detour(
    address = jc3gi::graphics_engine::render_engine::RenderEngine::SetAllGlobalShaderProgramConstants_ADDRESS
)]
fn set_all_global_shader_program_constants(this: *mut RenderEngine, ctx: *const c_void) {
    SET_ALL_GLOBAL_SHADER_PROGRAM_CONSTANTS
        .get()
        .unwrap()
        .call(this, ctx);
    if crate::stereo::single_pass::active()
        && let Some(engine) = unsafe { this.as_ref() }
    {
        crate::stereo::single_pass::mirror_and_bind_cb13(engine);
    }
}
