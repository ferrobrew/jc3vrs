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
use jc3gi::graphics_engine::graphics_engine::RenderContext;
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
        .with_static_binder(&SET_GLOBAL_SHADER_CONSTANTS_BINDER)
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

// RenderEngine::SetGlobalShaderConstants -- stages the per-eye render context into the cb0 GlobalConstants
// (the shadow-cascade transform among them). The cascade transform is baked center-camera-relative, but
// the material shader anchors the shadow lookup at the per-eye camera position (cb0[4]), shifting each
// eye's shadow by `M * (eyePos - centerPos)` -- the per-eye sun-shadow mismatch. Adding `M * delta` to the
// transform's translation (with `delta = eyePos - centerPos`, the offset the camera hook applies)
// re-anchors the lookup to center while leaving stereo geometry untouched. Patched before the original
// stages the constants; a zero delta (no per-eye offset) makes it a no-op.
#[detour(address = jc3gi::graphics_engine::render_engine::RenderEngine::SetGlobalShaderConstants_ADDRESS)]
fn set_global_shader_constants(this: *mut c_void, ctx: *mut c_void) {
    if Config::lock_query(|c| c.stereo.fix_shadow_cascade_anchor)
        && let Some(ctx) = unsafe { ctx.cast::<RenderContext>().as_mut() }
    {
        let delta = crate::stereo::STEREO_STATE.lock().shadow_anchor_delta;
        apply_shadow_cascade_anchor_fix(ctx, delta);
    }
    SET_GLOBAL_SHADER_CONSTANTS.get().unwrap().call(this, ctx);
}

/// Add `M * delta` to the cascade transform translation row (`m_Transform` row 3), where `M`'s columns
/// are the transform's three linear rows (row 0/1/2 -- cb0[45..47] in the shader). Re-anchors the
/// sun-shadow lookup from the per-eye camera back to the center camera the cascade map was fit to. The
/// full `float4` add is used; the linear rows' `.w` are 0 for the affine cascade transform, so the
/// translation's `.w` is unchanged.
fn apply_shadow_cascade_anchor_fix(ctx: &mut RenderContext, delta: [f32; 3]) {
    let m = &mut ctx.m_ShadowCascades.m_Transform.data;
    for i in 0..4 {
        m[12 + i] += delta[0] * m[i] + delta[1] * m[4 + i] + delta[2] * m[8 + i];
    }
}
