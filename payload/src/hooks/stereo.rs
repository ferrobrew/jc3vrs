//! Detours that gate per-frame engine state so it advances only once per *real* frame, even
//! though we render the scene twice (once per eye). See PLAN.md sections 5.2/5.3.
//!
//! Each gate is toggleable at runtime (debug UI) so the working combination can be found in-game.
//! Defaults follow the render-list investigation: gate auto-exposure (a legitimate per-eye
//! concern), but let SetupRenderFrameData / HandBackBuffers run on eye 1 -- they own the per-pass
//! render-block-item double-buffer swap and the constant-buffer recycle, which eye 1 needs too.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use detours_macro::detour;
use re_utilities::hook_library::HookLibrary;

use crate::TraceEvent;

/// Skip the auto-exposure update on eye 1 (frame-counted; would double-adapt). Default on.
pub static GATE_EXPOSURE: AtomicBool = AtomicBool::new(true);
/// Skip `SetupRenderFrameData` (RBI list swap/zero) on eye 1. Default off -- eye 1 needs its swap.
pub static GATE_SETUP_RENDER_FRAME_DATA: AtomicBool = AtomicBool::new(false);
/// Skip `HandBackBuffers` (constant-buffer recycle) on eye 1. Default off.
pub static GATE_HAND_BACK_BUFFERS: AtomicBool = AtomicBool::new(false);

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
        .with_static_binder(&SETUP_RENDER_FRAME_DATA_BINDER)
        .with_static_binder(&HAND_BACK_BUFFERS_BINDER)
        .with_static_binder(&SMOOTHED_EXPOSURE_UPDATE_BINDER)
        .with_static_binder(&CALC_HISTOGRAM_MID_BRIGHT_BINDER)
}

// CRenderPass::SetupRenderFrameData -- swaps each pass's add/draw render-block-item lists and zeroes
// the new add-list. Eye 1 needs this too; gating it leaves eye 1 drawing a partial, un-swapped list
// (the central "wedge"). Off by default.
#[detour(address = jc3gi::graphics_engine::render_pass::CRenderPass::SetupRenderFrameData_ADDRESS)]
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

// CConstantBufferPool::HandBackBuffers -- recycles last frame's constant buffers back to the free
// pool. Suppressing it on eye 1 starves the second render of constant buffers. Off by default.
#[detour(address = jc3gi::graphics_engine::render_pass::CConstantBufferPool::HandBackBuffers_ADDRESS)]
fn hand_back_buffers(this: *mut c_void) {
    let gated = gate(&GATE_HAND_BACK_BUFFERS);
    crate::trace_eye(TraceEvent::HandBackBuffers { gated });
    if gated {
        return;
    }
    HAND_BACK_BUFFERS.get().unwrap().call(this);
}

// CToneMappingEffect::SSmoothedExposure::Update -- N-frame exposure smoother (no dt term, so it
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
