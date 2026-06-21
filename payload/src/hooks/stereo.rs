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

use detours_macro::detour;
use jc3gi::graphics_engine::post_effects::PostEffectContext;
use jc3gi::graphics_engine::tone_mapping::{SHistogramGeneration, ToneMappingEffect};
use re_utilities::hook_library::HookLibrary;

use crate::config::Config;
use crate::stereo::is_second_eye;
use crate::trace::{TraceEvent, TraceState};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&ROTATE_RENDER_FRAME_DATA_BINDER)
        .with_static_binder(&SETUP_RENDER_FRAME_DATA_BINDER)
        .with_static_binder(&HAND_BACK_BUFFERS_BINDER)
        .with_static_binder(&SMOOTHED_EXPOSURE_UPDATE_BINDER)
        .with_static_binder(&CALC_HISTOGRAM_MID_BRIGHT_BINDER)
        .with_static_binder(&APPLY_WORLD_FILTERS_BINDER)
        .with_static_binder(&APPLY_GLOBAL_FILTERS_BINDER)
        .with_static_binder(&TONEMAPPING_UPDATE_BINDER)
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

// ToneMappingEffect::SSmoothedExposure::Update -- N-frame exposure smoother (no dt term, so it
// double-adapts when the scene renders twice per frame). Skip on eye 1; both eyes then share the
// first eye's exposure (which is what you want anyway -- no binocular rivalry).
#[detour(address = jc3gi::graphics_engine::tone_mapping::SSmoothedExposure::Update_ADDRESS)]
fn smoothed_exposure_update(this: *mut c_void, exposure: f32) {
    let gated = is_second_eye() && Config::lock_query(|c| c.exposure.gate);
    TraceState::record_eye(TraceEvent::SmoothedExposureUpdate { gated, exposure });
    if gated {
        return;
    }
    SMOOTHED_EXPOSURE_UPDATE.get().unwrap().call(this, exposure);
}

// CalculateMidAndBrightPointForHistogram -- the per-frame histogram percentile computation; Update
// calls it once per histogram. The exposure readback now happens in the Update detour (which holds
// the ToneMappingEffect directly and can read both histograms), so this only keeps the (inert) gate.
#[detour(address = jc3gi::graphics_engine::tone_mapping::CalculateMidAndBrightPointForHistogram_ADDRESS)]
fn calc_histogram_mid_bright(
    ctx: *mut c_void,
    arg1: f32,
    arg2: i32,
    arg3: f32,
    hist: *mut SHistogramGeneration,
) {
    let gated = is_second_eye() && Config::lock_query(|c| c.exposure.gate);
    TraceState::record_eye(TraceEvent::CalcHistogramMidBright { gated });
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
    let gated = is_second_eye() && Config::lock_query(|c| c.stereo.gate_eye1_dt);
    TraceState::record_eye(TraceEvent::ApplyWorldFilters { gated });
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
    let gated = is_second_eye() && Config::lock_query(|c| c.stereo.gate_eye1_dt);
    TraceState::record_eye(TraceEvent::ApplyGlobalFilters { gated });
    let dt = if gated { 0.0 } else { dt };
    APPLY_GLOBAL_FILTERS.get().unwrap().call(this, dt, ctx);
}

// ToneMappingEffect::Update -- the per-frame exposure step (CPostEffectsManager::UpdateRender -> here).
// With the real 3-arg signature this is the canonical place to (a) pin m_CurrentExposure for the A/B,
// and (b) read the exposure internals. The target divisor is m_Histogram2's mid-point -- what the
// converged exposure tracks, NOT m_Histogram. The whole exposure path is once-per-frame, so there is
// no per-eye gating to do here.
#[detour(address = jc3gi::graphics_engine::tone_mapping::ToneMappingEffect::Update_ADDRESS)]
fn tonemapping_update(
    this: *mut ToneMappingEffect,
    manager: *mut c_void,
    ctx: *mut PostEffectContext,
) {
    TONEMAPPING_UPDATE.get().unwrap().call(this, manager, ctx);
    let Some(tme) = (unsafe { this.as_mut() }) else {
        return;
    };
    let (force_exposure, forced_value) =
        Config::lock_query(|c| (c.exposure.force, c.exposure.forced_value));
    if force_exposure {
        tme.m_CurrentExposure = forced_value;
    }
    if crate::trace::tracing_active() {
        let target_num = unsafe { ctx.as_ref() }
            .map(|c| c.m_AutoExposureKey)
            .unwrap_or_default();
        let pingpong = tme.m_HistogramPingPong;
        let divisor = tme.m_Histogram2.m_HistogramMidPoint;
        let n = (tme.m_NumBuckets as usize).min(tme.m_Histogram.m_NumPixelsInBuckets.len());
        TraceState::record_eye(TraceEvent::ExposureInternals {
            exposure: tme.m_CurrentExposure,
            target_num,
            divisor,
            target: if divisor != 0.0 {
                target_num / divisor
            } else {
                0.0
            },
            hist1_bright: tme.m_Histogram.m_HistogramBrightPoint,
            hist1_mid: tme.m_Histogram.m_HistogramMidPoint,
            hist1_buckets: tme.m_Histogram.m_NumPixelsInBuckets[..n].to_vec(),
            hist2_bright: tme.m_Histogram2.m_HistogramBrightPoint,
            hist2_mid: tme.m_Histogram2.m_HistogramMidPoint,
            hist2_buckets: tme.m_Histogram2.m_NumPixelsInBuckets[..n].to_vec(),
            num_buckets: tme.m_NumBuckets,
            pingpong,
            forced: force_exposure,
        });
    }
}
