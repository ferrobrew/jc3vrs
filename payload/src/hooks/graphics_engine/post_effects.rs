//! Detours on the post-effects chain.
//!
//! Two concerns share this file. First, the fullscreen reprojection passes (motion blur, depth of
//! field) do a screen-space reprojection whose per-frame-once state is only valid for one camera, so
//! a second (stereo) render can paint a sub-region -- skip the whole pass (default on: VR wants both
//! off anyway, and it's a clean A/B for the eye-1 "wedge"). Each `Apply` returns its source slot
//! index, so returning `input` is a clean pass-through. Second, the manager-level filter enqueues
//! (`ApplyWorldFilters` / `ApplyGlobalFilters`) step dt-driven accumulators that must only advance
//! once per real frame, so their `dt` is zeroed on eye 1.

use std::ffi::c_void;

use detours_macro::detour;
use jc3gi::graphics_engine::post_effects::{
    AAMode, AntiAliasingEffect, PostEffectContext, PostEffectRenderFlags,
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    debug::trace::{TraceEvent, TraceState},
    stereo::{self, draw_index, is_second_eye},
};

pub(super) fn extend(library: HookLibrary) -> HookLibrary {
    library
        .with_static_binder(&APPLY_WORLD_FILTERS_BINDER)
        .with_static_binder(&APPLY_GLOBAL_FILTERS_BINDER)
        .with_static_binder(&MOTION_BLUR_APPLY_BINDER)
        .with_static_binder(&DOF_APPLY_BINDER)
        .with_static_binder(&FADE_APPLY_BINDER)
        .with_static_binder(&GLARE_APPLY_BINDER)
        .with_static_binder(&PLAYER_DAMAGE_APPLY_BINDER)
        .with_static_binder(&SUN_HALO_PRE_APPLY_BINDER)
        .with_static_binder(&SUN_HALO_APPLY_BINDER)
        .with_static_binder(&ANTI_ALIASING_APPLY_BINDER)
}

/// Dispatch FSR for the current eye, reading the chain's current slot color (`mgr[slot + 83]`) as
/// input and writing the anti-aliased result back into it. Returns whether FSR ran (in which case the
/// engine AA should be neutralized to a passthrough). Mirrors `capture_post_result`'s slot access.
fn fsr_dispatch(mgr: *mut c_void, slot: u32, sharpness: Option<f32>) -> bool {
    if mgr.is_null() {
        return false;
    }
    // SAFETY: at the AA stage `mgr[slot + 83]` is the chain's current result texture (the LDR,
    // post-tonemap color), and the engine buffers are live on the render thread.
    let ran = unsafe {
        let slot_color = (mgr as *const *mut jc3gi::graphics_engine::texture::Texture)
            .add(slot as usize + 83)
            .read();
        let Some(ge) = jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() else {
            return false;
        };
        let (Some(device), Some(color), Some(depth), Some(velocity)) = (
            ge.m_Device.as_ref(),
            slot_color.as_ref(),
            ge.m_MainDepthTexture.as_ref(),
            ge.m_VelocityBufferTexture.as_ref(),
        ) else {
            return false;
        };
        let mut state = crate::fsr::FSR_STATE.lock();
        crate::fsr::dispatch_eye(
            &mut state,
            device,
            draw_index(),
            color,
            depth,
            velocity,
            sharpness,
        )
    };
    TraceState::record_eye(TraceEvent::FsrDispatch { input: slot, ran });
    ran
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

// AntiAliasingEffect::Apply -- the AA stage of the post chain. Two roles:
//   * FSR on: resolve the slot color ourselves (post-tonemap) via fsr_dispatch, then neutralize the
//     engine AA to AA_NONE so it passthrough-blits our FSR result onward (docs/fsr.md).
//   * else, force-SMAA-1x in stereo: drop T2X (mode 3) to SMAA 1x (mode 2) -- T2X's temporal resolve
//     blends a single history shared across the two eye dispatches, so each eye ghosts the other.
// Either way, restore the real mode afterwards so the engine's own state stays intact.
#[detour(address = jc3gi::graphics_engine::post_effects::AntiAliasingEffect::Apply_ADDRESS)]
fn anti_aliasing_apply(
    this: *mut AntiAliasingEffect,
    ctx: *mut c_void,
    pec: *mut c_void,
    mgr: *mut c_void,
    slot: *mut u32,
) -> u64 {
    let (force_smaa_1x, fsr) = Config::lock_query(|c| {
        (
            stereo::active() && c.stereo.force_smaa_1x,
            c.fsr.enabled.then_some(c.fsr.sharpness),
        )
    });

    // When FSR is on, resolve into the current slot ourselves, then let the engine AA run as a
    // passthrough (AA_NONE blits the slot onward without filtering). FSR replaces SMAA, so its
    // 1x-forcing is moot here.
    let saved_mode = unsafe { (*this).m_Mode };
    let mut restore = false;
    if let Some(sharpness) = fsr {
        // SAFETY: `slot` is a valid out-param holding the current chain slot index.
        let slot_index = unsafe { *slot };
        if fsr_dispatch(mgr, slot_index, sharpness) {
            unsafe { (*this).m_Mode = AAMode::AA_NONE };
            restore = true;
        }
    } else if force_smaa_1x && saved_mode == AAMode::AA_SMAA_T2X {
        // T2X's temporal resolve blends a single shared history across the two eye dispatches, so each
        // eye ghosts the other; 1x carries no history.
        unsafe { (*this).m_Mode = AAMode::AA_SMAA };
        restore = true;
    }

    let r = ANTI_ALIASING_APPLY
        .get()
        .unwrap()
        .call(this, ctx, pec, mgr, slot);
    if restore {
        unsafe { (*this).m_Mode = saved_mode };
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
        crate::ui::render::capture_post_stage(stage, eye, result);
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
    TraceState::record_eye(TraceEvent::MotionBlurApply {
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
    capture_post_result(crate::ui::render::POST_STAGE_MB, mgr, slot);
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
    TraceState::record_eye(TraceEvent::DofApply {
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
    capture_post_result(crate::ui::render::POST_STAGE_DOF, mgr, slot);
    slot
}

// FadeEffect::Apply -- alpha-blended fade quad. Skip = no-op (the u64 return is discarded).
#[detour(address = jc3gi::graphics_engine::post_effects::FadeEffect::Apply_ADDRESS)]
fn fade_apply(this: *mut c_void, a2: *mut c_void, a3: *mut c_void) -> u64 {
    let skip = Config::lock_query(|c| c.post_fx.skip_fade);
    TraceState::record_eye(TraceEvent::FadeApply { skip });
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
    TraceState::record_eye(TraceEvent::GlareApply { skip });
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
    TraceState::record_eye(TraceEvent::PlayerDamageApply { input, skip });
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
    TraceState::record_eye(TraceEvent::SunHaloPreApply { skip });
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
    TraceState::record_eye(TraceEvent::SunHaloApply { skip });
    if skip {
        return 0;
    }
    SUN_HALO_APPLY.get().unwrap().call(this, a2)
}
