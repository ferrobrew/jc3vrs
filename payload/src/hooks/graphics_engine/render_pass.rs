//! Detours that gate per-frame render-list state so it advances only once per *real* frame, even
//! though we render the scene twice (once per eye). See PLAN.md sections 5.2/5.3.
//!
//! Each gate is toggleable at runtime (debug UI) so the working combination can be found in-game.
//! `SetupRenderFrameData` (the per-batch list *build*, not the swap) and `HandBackBuffers`
//! (constant-buffer recycle) run on both eyes. The add/draw list parity is handled separately by
//! saving and restoring `current_add_buffer` in `game::game_update_render` between eyes, so the
//! per-frame `CKeep1000Frames` call (which toggles the parity and calls `SaveRenderFrameData` on
//! every pass) runs on both eyes and produces the same list pointers on both.

use std::ffi::c_void;

use detours_macro::detour;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    debug::trace::{TraceEvent, TraceState},
    stereo::is_second_eye,
};

pub(super) fn extend(library: HookLibrary) -> HookLibrary {
    library
        .with_static_binder(&SETUP_RENDER_FRAME_DATA_BINDER)
        .with_static_binder(&HAND_BACK_BUFFERS_BINDER)
}

// RenderPass::SetupRenderFrameData -- the per-batch list *build*: appends `count` render-block-items
// to the active add-list. Runs on worker threads during the sim, not during our Draw calls, so the
// eye-1 gate never actually fires; it is NOT the add/draw swap (the swap is handled by restoring
// `current_add_buffer` between eyes in `game::game_update_render`).
#[detour(address = jc3gi::graphics_engine::render_pass::RenderPass::SetupRenderFrameData_ADDRESS)]
fn setup_render_frame_data(a1: *mut c_void, count: i32, a3: *mut c_void, items: *mut c_void) {
    let gated = is_second_eye() && Config::lock_query(|c| c.stereo.gate_setup_render_frame_data);
    TraceState::record_eye(TraceEvent::SetupRenderFrameData { gated });
    if gated {
        return;
    }
    SETUP_RENDER_FRAME_DATA
        .get()
        .unwrap()
        .call(a1, count, a3, items);
}

// ConstantBufferPool::HandBackBuffers -- recycles last frame's constant buffers back to the free
// pool. Suppressing it on eye 1 starves the second render of constant buffers. Off by default.
#[detour(address = jc3gi::graphics_engine::render_pass::ConstantBufferPool::HandBackBuffers_ADDRESS)]
fn hand_back_buffers(this: *mut c_void) {
    let gated = is_second_eye() && Config::lock_query(|c| c.stereo.gate_hand_back_buffers);
    TraceState::record_eye(TraceEvent::HandBackBuffers { gated });
    if gated {
        return;
    }
    HAND_BACK_BUFFERS.get().unwrap().call(this);
}
