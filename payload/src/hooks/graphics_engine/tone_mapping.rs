//! Detours on the auto-exposure / tone-mapping path.
//!
//! The exposure pipeline runs once per scene dispatch, so a stereo (twice-per-frame) render
//! double-adapts and over-meters unless the per-eye work is gated to eye 0. These hooks pin the
//! exposure for the A/B, gate the smoother and the histogram metering on eye 1, and read the
//! exposure internals for tracing.

use std::ffi::c_void;

use detours_macro::detour;
use jc3gi::graphics_engine::{
    post_effects::PostEffectContext,
    tone_mapping::{SHistogramGeneration, ToneMappingEffect},
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    stereo::{draw_index, is_second_eye},
    trace::{TraceEvent, TraceState},
};

pub(super) fn extend(library: HookLibrary) -> HookLibrary {
    library
        .with_static_binder(&SMOOTHED_EXPOSURE_UPDATE_BINDER)
        .with_static_binder(&CALC_HISTOGRAM_MID_BRIGHT_BINDER)
        .with_static_binder(&TONEMAPPING_UPDATE_BINDER)
        .with_static_binder(&GENERATE_HISTOGRAM_BINDER)
        .with_static_binder(&DRAW_HISTOGRAM_WINDOW_BINDER)
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

// ToneMappingEffect::GenerateHistogramForFinalScene -- auto-exposure histogram. Skipping stalls
// adaptation, but preserve the out-param contract (this+764/765 = current slot indices) so callers
// don't read garbage. A bisection aid for the stereo darkening.
#[detour(
    address = jc3gi::graphics_engine::tone_mapping::ToneMappingEffect::GenerateHistogramForFinalScene_ADDRESS
)]
fn generate_histogram(
    this: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
    a4: *mut c_void,
    a5: i32,
    a6: *mut u32,
    a7: *mut u32,
) -> *mut u32 {
    // Gate the histogram *population* on eye 1 when stereo, symmetric to the exposure-*adaptation*
    // gate (exposure.gate). Both eyes otherwise write the GPU luminance buckets, so the gated
    // per-frame exposure Update reads a histogram populated by both dispatches and settles too dark.
    let (skip_histogram, gate_exposure) =
        Config::lock_query(|c| (c.post_fx.skip_histogram, c.exposure.gate));
    let eye1_gated = is_second_eye() && gate_exposure;
    let skip = skip_histogram || eye1_gated;
    TraceState::record_eye(TraceEvent::GenerateHistogram { skip });
    // The histogram reads the final HDR scene (MainColor) for auto-exposure, so this is the first
    // point in the post chain where MainColor still holds this dispatch's clean scene -- grab it for
    // the per-eye "Scene" preview before the chain recycles it.
    crate::capture_main_color(draw_index());

    if skip {
        unsafe {
            let base = this as *const u32;
            if !a6.is_null() {
                *a6 = *base.add(764);
            }
            if !a7.is_null() {
                *a7 = *base.add(765);
            }
        }
        return a7;
    }
    GENERATE_HISTOGRAM
        .get()
        .unwrap()
        .call(this, a2, a3, a4, a5, a6, a7)
}

// ToneMappingEffect::DrawHistogramWindow -- despite the name, this meters the *second*, un-exposure-
// weighted histogram (m_Histogram2): the raw scene brightness that Update divides the auto-exposure
// target by. Like GenerateHistogramForFinalScene it runs once per dispatch, but only that first meter
// was gated -- so in stereo m_Histogram2 was metered on both eyes, its occlusion-query ring inflated
// and corrupted, and the exposure divided by a too-large brightness => the frame went dark. Gate it to
// eye 0 (under GATE_EXPOSURE), symmetric with generate_histogram, so it meters once per real frame.
#[detour(
    address = jc3gi::graphics_engine::tone_mapping::ToneMappingEffect::DrawHistogramWindow_ADDRESS
)]
fn draw_histogram_window(
    this: *mut c_void,
    ctx: *mut c_void,
    pec: *mut c_void,
    mgr: *mut c_void,
    index: u32,
) {
    let eye1_gated = is_second_eye() && Config::lock_query(|c| c.exposure.gate);
    TraceState::record_eye(TraceEvent::DrawHistogramWindow { skip: eye1_gated });
    if eye1_gated {
        return;
    }
    DRAW_HISTOGRAM_WINDOW
        .get()
        .unwrap()
        .call(this, ctx, pec, mgr, index);
}
