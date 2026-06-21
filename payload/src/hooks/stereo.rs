//! Detours that gate per-frame engine state so it advances only once per *real* frame, even
//! though we render the scene twice (once per eye). See PLAN.md sections 5.2/5.3.
//!
//! Each gate is toggleable at runtime (debug UI) so the working combination can be found in-game.
//! Defaults follow the render-list investigation: gate auto-exposure (a legitimate per-eye concern)
//! and the per-frame render-list rotation (`RotateRenderFrameData` -- the real add/draw double-buffer
//! flip; skipping it on eye 1 keeps eye 1 on eye 0's populated, non-destructively-drawn lists, which
//! is the core stereo-geometry fix). `SetupRenderFrameData` (the per-batch list *build*, not the
//! swap) and `HandBackBuffers` (constant-buffer recycle) run on both eyes.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use detours_macro::detour;
use re_utilities::hook_library::HookLibrary;

use crate::TraceEvent;

/// Skip the auto-exposure update on eye 1 (frame-counted; would double-adapt). Default on.
pub static GATE_EXPOSURE: AtomicBool = AtomicBool::new(true);
/// Skip the per-frame render-list rotation (`RotateRenderFrameData`) on eye 1, so eye 1 reuses eye
/// 0's populated draw lists instead of flipping to the just-emptied buffer. Default on -- this is the
/// core stereo geometry fix (eye 1 was drawing zero render blocks without it).
pub static GATE_ROTATE_RENDER_FRAME_DATA: AtomicBool = AtomicBool::new(true);
/// Skip `SetupRenderFrameData` (the per-batch RBI list *build*) on eye 1. Default off. NOTE: this is
/// the list build, not the swap (that is `RotateRenderFrameData`); it runs during the sim, so gating
/// it would starve both eyes' lists. Kept only for experimentation.
pub static GATE_SETUP_RENDER_FRAME_DATA: AtomicBool = AtomicBool::new(false);
/// Skip `HandBackBuffers` (constant-buffer recycle) on eye 1. Default off.
pub static GATE_HAND_BACK_BUFFERS: AtomicBool = AtomicBool::new(false);
/// Force the post-effect `dt` to 0 on the eye-1 dispatch so the dt-driven accumulators (world fade,
/// screen-fade alpha, sun-direction / heat-haze) advance once per real frame instead of twice
/// (otherwise fades run at ~2x and the sun/haze shimmer runs fast). Default on.
pub static GATE_EYE1_DT: AtomicBool = AtomicBool::new(true);

/// True while the manual Draw driver is rendering the *second* eye.
fn is_second_eye() -> bool {
    crate::STEREO.load(Ordering::Relaxed) && crate::DRAW_INDEX.load(Ordering::Relaxed) == 1
}

/// Whether this eye-1 pass should skip the gated call (i.e. we're on eye 1 and the gate is on).
fn gate(flag: &AtomicBool) -> bool {
    is_second_eye() && flag.load(Ordering::Relaxed)
}

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&ROTATE_RENDER_FRAME_DATA_BINDER)
        .with_static_binder(&SETUP_RENDER_FRAME_DATA_BINDER)
        .with_static_binder(&HAND_BACK_BUFFERS_BINDER)
        .with_static_binder(&SMOOTHED_EXPOSURE_UPDATE_BINDER)
        .with_static_binder(&CALC_HISTOGRAM_MID_BRIGHT_BINDER)
        .with_static_binder(&APPLY_WORLD_FILTERS_BINDER)
        .with_static_binder(&APPLY_GLOBAL_FILTERS_BINDER)
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
    let gated = gate(&GATE_ROTATE_RENDER_FRAME_DATA);
    crate::trace_eye(TraceEvent::RotateRenderFrameData { gated });
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
    let gated = gate(&GATE_SETUP_RENDER_FRAME_DATA);
    crate::trace_eye(TraceEvent::SetupRenderFrameData { gated });
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
    let gated = gate(&GATE_HAND_BACK_BUFFERS);
    crate::trace_eye(TraceEvent::HandBackBuffers { gated });
    if gated {
        return;
    }
    HAND_BACK_BUFFERS.get().unwrap().call(this);
}

// ToneMappingEffect::SSmoothedExposure::Update -- N-frame exposure smoother (no dt term, so it
// double-adapts when the scene renders twice per frame). Skip on eye 1; both eyes then share the
// first eye's exposure (which is what you want anyway -- no binocular rivalry).
#[detour(address = jc3gi::graphics_engine::tone_mapping::SSmoothedExposure::Update_ADDRESS)]
fn smoothed_exposure_update(this: *mut c_void, exposure: f32) {
    let gated = gate(&GATE_EXPOSURE);
    crate::trace_eye(TraceEvent::SmoothedExposureUpdate { gated, exposure });
    if gated {
        return;
    }
    SMOOTHED_EXPOSURE_UPDATE.get().unwrap().call(this, exposure);
}

// CalculateMidAndBrightPointForHistogram -- the per-frame eye-adaptation lerp (also frame-counted).
// Same treatment: only advance for the first eye.
#[detour(address = jc3gi::graphics_engine::tone_mapping::CalculateMidAndBrightPointForHistogram_ADDRESS)]
fn calc_histogram_mid_bright(ctx: *mut c_void, arg1: f32, arg2: i32, arg3: f32, hist: *mut c_void) {
    let gated = gate(&GATE_EXPOSURE);
    crate::trace_eye(TraceEvent::CalcHistogramMidBright { gated });
    if gated {
        return;
    }
    CALC_HISTOGRAM_MID_BRIGHT
        .get()
        .unwrap()
        .call(ctx, arg1, arg2, arg3, hist);
}

// PostEffectsManager::ApplyWorldFilters -- enqueues the world post block and steps the world-fade
// accumulator (ApplyWorldFadeFilter) by `dt`. Zero `dt` on eye 1 so the fade advances once per real
// frame, not twice.
#[detour(address = jc3gi::graphics_engine::post_effects::PostEffectsManager::ApplyWorldFilters_ADDRESS)]
#[allow(clippy::too_many_arguments)]
fn apply_world_filters(
    this: *mut c_void,
    dt: f32,
    setup: *mut c_void,
    a4: *mut c_void,
    a5: *mut c_void,
    a6: *mut c_void,
    a7: *mut c_void,
    a8: *mut c_void,
) {
    let gated = gate(&GATE_EYE1_DT);
    crate::trace_eye(TraceEvent::ApplyWorldFilters { gated });
    let dt = if gated { 0.0 } else { dt };
    APPLY_WORLD_FILTERS
        .get()
        .unwrap()
        .call(this, dt, setup, a4, a5, a6, a7, a8);
}

// PostEffectsManager::ApplyGlobalFilters -- enqueues the global post block and steps its dt-driven
// accumulators (screen-fade alpha, sun-direction / heat-haze). Zero `dt` on eye 1 for the same
// once-per-real-frame reason.
#[detour(address = jc3gi::graphics_engine::post_effects::PostEffectsManager::ApplyGlobalFilters_ADDRESS)]
fn apply_global_filters(this: *mut c_void, dt: f32, ctx: *mut c_void) {
    let gated = gate(&GATE_EYE1_DT);
    crate::trace_eye(TraceEvent::ApplyGlobalFilters { gated });
    let dt = if gated { 0.0 } else { dt };
    APPLY_GLOBAL_FILTERS.get().unwrap().call(this, dt, ctx);
}
