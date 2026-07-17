//! Profiler-only detours (issue #34), compiled under the `profiler` feature.
//!
//! These hooks exist purely to instrument the game; they add no behaviour of their own beyond
//! opening puffin CPU scopes and GPU timestamp brackets. The frame phases and render seams that
//! the mod *already* hooks for other reasons are instrumented inline in those hooks (also under
//! the feature); this module covers the seams that are otherwise untouched:
//!
//! - `RenderEngine::DrawGBuffer` and `RenderEngine::Draw` — the two scene seams between `PreDraw`
//!   and `DrawPosteffects`, bracketed on the GPU timeline and timed on the CPU.
//! - `CGame::UpdateGame` — one sim tick (the render phases are covered by `game_update_render`).
//! - `CRenderPass::ChangeRenderBlockType` — opens a per-render-block-type CPU scope named by the
//!   type, closed by the next switch or by the pass draw's tail (see `render_pass::do_draw`).
//!
//! The engine's own scope-marker calls (`Graphics::BeginScopeMarker` / `EndScopeMarker`, §1.5 of
//! `docs/engine/profiling.md`) are *not* detoured: they are three-byte `ret` stubs, too small to
//! hook without risking the shared trampoline, and a failed detour would abort the whole hook
//! library. The per-render-block-type names they would have carried are recovered from
//! `ChangeRenderBlockType` above instead.

use detours_macro::detour;
use jc3gi::{
    game::{Game, UpdateContexts},
    graphics_engine::{
        graphics_engine::{HContext_t, HTexture_t, RenderContext},
        render_engine::{RenderBlockTypeBase, RenderEngine},
        render_pass::RenderPass,
    },
};
use re_utilities::hook_library::HookLibrary;

use crate::profiler::{
    self,
    gpu::{self, GpuSeam},
    type_scope,
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&DRAW_GBUFFER_BINDER)
        .with_static_binder(&RENDER_ENGINE_DRAW_BINDER)
        .with_static_binder(&UPDATE_GAME_BINDER)
        .with_static_binder(&CHANGE_RENDER_BLOCK_TYPE_BINDER)
}

// RenderEngine::DrawGBuffer -- the GBuffer fill (depth/velocity prefix, models, decals). The first
// scene seam after PreDraw; bracketed on the GPU timeline.
#[detour(address = jc3gi::graphics_engine::render_engine::RenderEngine::DrawGBuffer_ADDRESS)]
fn draw_gbuffer(this: *mut RenderEngine, ctx: *mut HContext_t, a3: i64, a4: *mut HTexture_t) {
    puffin::profile_scope!("DrawGBuffer");
    // SAFETY: `ctx` is the live immediate context for this dispatch.
    let _gpu = unsafe { gpu::seam(ctx, GpuSeam::GBuffer) };
    DRAW_GBUFFER.get().unwrap().call(this, ctx, a3, a4);
}

// RenderEngine::Draw -- lighting, reflections, opaque, environment, water, transparency. The second
// scene seam; bracketed on the GPU timeline.
#[detour(address = jc3gi::graphics_engine::render_engine::RenderEngine::Draw_ADDRESS)]
fn render_engine_draw(this: *mut RenderEngine, ctx: *mut HContext_t) -> u64 {
    puffin::profile_scope!("Draw (scene)");
    // SAFETY: `ctx` is the live immediate context for this dispatch.
    let _gpu = unsafe { gpu::seam(ctx, GpuSeam::Scene) };
    RENDER_ENGINE_DRAW.get().unwrap().call(this, ctx)
}

// CGame::UpdateGame -- one sim tick (clock, FPS, active game-state update). Called zero or more
// times per real frame per the frame-pacing mode, so the flame graph shows how many sim ticks a
// frame ran and where their time went.
#[detour(address = jc3gi::game::Game::UpdateGame_ADDRESS)]
fn update_game(this: *mut Game, update_contexts: *mut UpdateContexts) {
    puffin::profile_scope!("CGame::UpdateGame");
    UPDATE_GAME.get().unwrap().call(this, update_contexts);
}

// CRenderPass::ChangeRenderBlockType -- switches the active render-block type between type runs in
// a pass draw. Mirror it into a per-thread type scope: closing `prev`'s scope and opening `next`'s,
// named by the type. The final run's scope is closed by `render_pass::do_draw` at the pass tail.
#[detour(address = jc3gi::graphics_engine::render_pass::RenderPass::ChangeRenderBlockType_ADDRESS)]
fn change_render_block_type(
    this: *mut RenderPass,
    ctx: *mut RenderContext,
    prev: *mut RenderBlockTypeBase,
    next: *mut RenderBlockTypeBase,
    inout_count: *mut u32,
) {
    if profiler::are_scopes_on() {
        // End the previous run's scope *before* the next one begins: puffin streams are strictly
        // LIFO per thread, so begin-before-end here would nest the runs into each other and garble
        // the pass's whole subtree.
        type_scope::clear();
        // SAFETY: `next`, when non-null, is a live render-block type with a valid vtable; its type
        // name is a static string in the module image.
        let scope = unsafe {
            next.as_ref()
                .and_then(|ty| ty.get_type_name_str())
                .and_then(profiler::scope_for_name)
        };
        type_scope::replace(scope);
    }
    CHANGE_RENDER_BLOCK_TYPE
        .get()
        .unwrap()
        .call(this, ctx, prev, next, inout_count);
}
