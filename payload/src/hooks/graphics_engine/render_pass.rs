//! Detours that gate per-frame render-list state so it advances only once per *real* frame, even
//! though we render the scene twice (once per eye). See PLAN.md sections 5.2/5.3.
//!
//! Each gate is toggleable at runtime (debug UI) so the working combination can be found in-game.
//! Defaults follow the render-list investigation: gate the per-frame render-list rotation
//! (`RotateRenderFrameData` -- the real add/draw double-buffer flip; skipping it on eye 1 keeps eye 1
//! on eye 0's populated, non-destructively-drawn lists, which is the core stereo-geometry fix).
//! `SetupRenderFrameData` (the per-batch list *build*, not the swap) and `HandBackBuffers`
//! (constant-buffer recycle) run on both eyes.

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
        .with_static_binder(&ROTATE_RENDER_FRAME_DATA_BINDER)
        .with_static_binder(&SETUP_RENDER_FRAME_DATA_BINDER)
        .with_static_binder(&HAND_BACK_BUFFERS_BINDER)
}

// RotateRenderFrameData -- the per-frame render-block-item list rotation, run in each
// CGraphicsEngine::Draw prologue. It toggles the global add/draw parity and, for every render pass,
// re-points m_CurrentAddList/m_CurrentDrawList to the new buffer and zeroes the new add-list (then
// flushes the overflow list). Run twice (once per eye), eye 1's rotation flips the draw list back to
// the buffer eye 0 just zeroed -- so eye 1 draws zero render blocks. Skipping it on eye 1 keeps eye 1
// on eye 0's populated draw lists; DoDraw is non-destructive, so the render blocks redraw
// identically. On by default -- this is the core stereo-geometry fix.
#[detour(address = jc3gi::graphics_engine::render_pass::RotateRenderFrameData_ADDRESS)]
fn rotate_render_frame_data() {
    let gated = is_second_eye() && Config::lock_query(|c| c.stereo.gate_rotate_render_frame_data);
    TraceState::record_eye(TraceEvent::RotateRenderFrameData { gated });
    if gated {
        return;
    }
    ROTATE_RENDER_FRAME_DATA.get().unwrap().call();
}

// RenderPass::SetupRenderFrameData -- the per-batch list *build*: appends `count` render-block-items
// to the active add-list. Runs on worker threads during the sim, not during our Draw calls, so the
// eye-1 gate never actually fires; it is NOT the add/draw swap (see rotate_render_frame_data above).
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
