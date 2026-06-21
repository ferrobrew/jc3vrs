//! Detours that skip the fullscreen reprojection post-effects (motion blur, depth of field).
//!
//! Both passes do a screen-space reprojection whose per-frame-once state is only valid for one
//! camera, so a second (stereo) render can paint a sub-region. They run regardless of the camera's
//! blur weights (which only gate the recon/radial blur strength), so skip the whole pass. Default
//! on: VR wants both off anyway, and it's a clean A/B for the eye-1 "wedge". Each `Apply` returns
//! its source slot index, so returning `input` is a clean pass-through.

use std::ffi::c_void;

use detours_macro::detour;
use jc3gi::graphics_engine::post_effects::{
    AAMode, AntiAliasingEffect, PostEffectContext, PostEffectRenderFlags,
};
use re_utilities::hook_library::HookLibrary;

use crate::TraceEvent;
use crate::config::Config;
use crate::stereo::{self, draw_index, is_second_eye};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&MOTION_BLUR_APPLY_BINDER)
        .with_static_binder(&DOF_APPLY_BINDER)
        .with_static_binder(&FADE_APPLY_BINDER)
        .with_static_binder(&GLARE_APPLY_BINDER)
        .with_static_binder(&PLAYER_DAMAGE_APPLY_BINDER)
        .with_static_binder(&SUN_HALO_PRE_APPLY_BINDER)
        .with_static_binder(&SUN_HALO_APPLY_BINDER)
        .with_static_binder(&GENERATE_HISTOGRAM_BINDER)
        .with_static_binder(&DRAW_HISTOGRAM_WINDOW_BINDER)
        .with_static_binder(&ANTI_ALIASING_APPLY_BINDER)
}

// AntiAliasingEffect::Apply -- drop SMAA T2X (mode 3) to SMAA 1x (mode 2) for the pass when forcing
// 1x in stereo. T2X's temporal resolve blends a previous-frame history that is a single buffer shared
// (ping-ponged) across the two eye dispatches, so each eye blends the other -> a cross-eye ghost. 1x
// carries no history. Restore the real mode afterwards so the engine's own state stays intact.
#[detour(address = jc3gi::graphics_engine::post_effects::AntiAliasingEffect::Apply_ADDRESS)]
fn anti_aliasing_apply(
    this: *mut AntiAliasingEffect,
    ctx: *mut c_void,
    pec: *mut c_void,
    mgr: *mut c_void,
    slot: *mut u32,
) -> u64 {
    let force_smaa_1x = stereo::active() && Config::lock_query(|c| c.stereo.force_smaa_1x);
    let restore = unsafe {
        if force_smaa_1x && (*this).m_Mode == AAMode::AA_SMAA_T2X {
            (*this).m_Mode = AAMode::AA_SMAA;
            true
        } else {
            false
        }
    };
    let r = ANTI_ALIASING_APPLY
        .get()
        .unwrap()
        .call(this, ctx, pec, mgr, slot);
    if restore {
        unsafe { (*this).m_Mode = AAMode::AA_SMAA_T2X };
    }
    r
}

/// Read a stage's slot result texture (`CTX[slot+83]`, where `CTX` is the manager arg) and capture
/// it for the current eye into the debug overlay's per-stage preview.
fn capture_post_result(stage: usize, mgr: *mut c_void, slot: u32) {
    if mgr.is_null() {
        return;
    }
    let eye = draw_index();
    unsafe {
        let result = (mgr as *const *mut jc3gi::graphics_engine::texture::Texture)
            .add(slot as usize + 83)
            .read();
        crate::capture_post_stage(stage, eye, result);
    }
}

#[detour(address = jc3gi::graphics_engine::post_effects::MotionBlurEffect::Apply_ADDRESS)]
#[allow(clippy::too_many_arguments)]
fn motion_blur_apply(
    this: *mut c_void,
    ctx: *mut c_void,
    pec: *mut c_void,
    mgr: *mut c_void,
    input: u32,
    blur: f32,
    flag0: bool,
    flag1: bool,
) -> u32 {
    let (skip_motion_blur, skip_recon) =
        Config::lock_query(|c| (c.post_fx.skip_motion_blur, c.post_fx.skip_motion_blur_recon));
    crate::trace_eye(TraceEvent::MotionBlurApply {
        input,
        skip: skip_motion_blur,
    });
    let slot = if skip_motion_blur {
        input
    } else {
        // `flag0` (a7) gates the reconstruction-filter motion blur (`if g_EnableMotionBlur && !a7`).
        // Forcing it true skips that reprojection blur -- the flicker -- but keeps the composite draw.
        let flag0 = flag0 || skip_recon;
        MOTION_BLUR_APPLY
            .get()
            .unwrap()
            .call(this, ctx, pec, mgr, input, blur, flag0, flag1)
    };
    capture_post_result(crate::POST_STAGE_MB, mgr, slot);
    slot
}

#[detour(address = jc3gi::graphics_engine::post_effects::DepthOfFieldEffect::Apply_ADDRESS)]
fn dof_apply(
    this: *mut c_void,
    ctx: *mut c_void,
    pec: *mut PostEffectContext,
    mgr: *mut c_void,
    input: u32,
) -> u32 {
    let (skip_dof, dof_no_reproject) =
        Config::lock_query(|c| (c.post_fx.skip_dof, c.post_fx.dof_no_reproject));
    crate::trace_eye(TraceEvent::DofApply {
        input,
        skip: skip_dof,
    });
    if skip_dof {
        return input;
    }
    let slot = if dof_no_reproject {
        // DoF's motion-vector reprojection (the flicker) is gated by bit 0 of the render context's
        // flags, inside its full composite/grade branch. Clear just that bit for the call so DoF
        // still grades the scene but skips the reprojection, then restore.
        let rc = unsafe { pec.as_mut().and_then(|p| p.m_RenderContext.as_mut()) };
        if let Some(rc) = rc {
            let saved = rc.m_Flags;
            rc.m_Flags
                .remove(PostEffectRenderFlags::m_MotionVectorReprojection);
            let result = DOF_APPLY.get().unwrap().call(this, ctx, pec, mgr, input);
            rc.m_Flags = saved;
            result
        } else {
            DOF_APPLY.get().unwrap().call(this, ctx, pec, mgr, input)
        }
    } else {
        DOF_APPLY.get().unwrap().call(this, ctx, pec, mgr, input)
    };
    capture_post_result(crate::POST_STAGE_DOF, mgr, slot);
    slot
}

// FadeEffect::Apply -- alpha-blended fade quad. Skip = no-op (the u64 return is discarded).
#[detour(address = jc3gi::graphics_engine::post_effects::FadeEffect::Apply_ADDRESS)]
fn fade_apply(this: *mut c_void, a2: *mut c_void, a3: *mut c_void) -> u64 {
    let skip = Config::lock_query(|c| c.post_fx.skip_fade);
    crate::trace_eye(TraceEvent::FadeApply { skip });
    if skip {
        return 0;
    }
    FADE_APPLY.get().unwrap().call(this, a2, a3)
}

// GlareEffect::Apply -- bloom/glare generator. Skip = no bloom.
#[detour(address = jc3gi::graphics_engine::post_effects::GlareEffect::Apply_ADDRESS)]
fn glare_apply(
    this: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
    a4: *mut c_void,
    a5: *mut c_void,
) -> u64 {
    let skip = Config::lock_query(|c| c.post_fx.skip_glare);
    crate::trace_eye(TraceEvent::GlareApply { skip });
    if skip {
        return 0;
    }
    GLARE_APPLY.get().unwrap().call(this, a2, a3, a4, a5)
}

// PlayerDamageEffect::Apply -- red damage vignette. Slot-passthrough; skip = return input slot.
#[detour(address = jc3gi::graphics_engine::post_effects::PlayerDamageEffect::Apply_ADDRESS)]
fn player_damage_apply(
    this: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
    a4: *mut c_void,
    input: u32,
) -> u32 {
    let skip = Config::lock_query(|c| c.post_fx.skip_player_damage);
    crate::trace_eye(TraceEvent::PlayerDamageApply { input, skip });
    if skip {
        return input;
    }
    PLAYER_DAMAGE_APPLY
        .get()
        .unwrap()
        .call(this, a2, a3, a4, input)
}

// SunHaloEffect::PreApply -- prepares the halo. Skip = clear the ready flag (this+0x114) so the
// paired Apply early-outs, then no-op.
#[detour(address = jc3gi::graphics_engine::post_effects::SunHaloEffect::PreApply_ADDRESS)]
fn sun_halo_pre_apply(this: *mut c_void, a2: *mut c_void, a3: *mut c_void, a4: *mut c_void) -> u64 {
    let skip = Config::lock_query(|c| c.post_fx.skip_sun_halo);
    crate::trace_eye(TraceEvent::SunHaloPreApply { skip });
    if skip {
        unsafe {
            *(this as *mut u8).add(0x114) = 0;
        }
        return 0;
    }
    SUN_HALO_PRE_APPLY.get().unwrap().call(this, a2, a3, a4)
}

// SunHaloEffect::Apply -- composites the halo additively. Skip = no-op.
#[detour(address = jc3gi::graphics_engine::post_effects::SunHaloEffect::Apply_ADDRESS)]
fn sun_halo_apply(this: *mut c_void, a2: *mut c_void) -> u64 {
    let skip = Config::lock_query(|c| c.post_fx.skip_sun_halo);
    crate::trace_eye(TraceEvent::SunHaloApply { skip });
    if skip {
        return 0;
    }
    SUN_HALO_APPLY.get().unwrap().call(this, a2)
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
    crate::trace_eye(TraceEvent::GenerateHistogram { skip });
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
    crate::trace_eye(TraceEvent::DrawHistogramWindow { skip: eye1_gated });
    if eye1_gated {
        return;
    }
    DRAW_HISTOGRAM_WINDOW
        .get()
        .unwrap()
        .call(this, ctx, pec, mgr, index);
}
