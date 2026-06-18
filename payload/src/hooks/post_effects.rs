//! Detours that skip the fullscreen reprojection post-effects (motion blur, depth of field).
//!
//! Both passes do a screen-space reprojection whose per-frame-once state is only valid for one
//! camera, so a second (stereo) render can paint a sub-region. They run regardless of the camera's
//! blur weights (which only gate the recon/radial blur strength), so skip the whole pass. Default
//! on: VR wants both off anyway, and it's a clean A/B for the eye-1 "wedge". Each `Apply` returns
//! its source slot index, so returning `input` is a clean pass-through.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use detours_macro::detour;
use jc3gi::graphics_engine::post_effects::{PostEffectContext, PostEffectRenderFlags};
use re_utilities::hook_library::HookLibrary;

use crate::TraceEvent;

/// Skip the *whole* MotionBlur pass. It is not the composite (DoF is), and it reprojects (flicker),
/// so skip it. Default on.
pub static SKIP_MOTION_BLUR: AtomicBool = AtomicBool::new(true);
/// Force `a7=true` so `ApplyReconstructionFilterMotionBlur` is skipped while MotionBlur's first
/// draw still runs (only relevant if the whole pass isn't skipped). Default on.
pub static SKIP_MOTION_BLUR_RECON: AtomicBool = AtomicBool::new(true);
/// Skip the *whole* DepthOfField pass. WARNING: DoF does the scene composite -- skipping it washes
/// the image out. Default off; use `DOF_NO_REPROJECT` instead.
pub static SKIP_DOF: AtomicBool = AtomicBool::new(false);
/// Clear DoF's motion-vector reprojection sub-gate (bit 0 of `(*pec)+0x384`) for the call. This
/// keeps DoF's full composite/grade branch but skips the screen-space reprojection that flickers.
pub static DOF_NO_REPROJECT: AtomicBool = AtomicBool::new(true);

// Per-stage skip toggles (bisection aids; default off = run normally).
/// Skip the fade quad.
pub static SKIP_FADE: AtomicBool = AtomicBool::new(false);
/// Skip the glare / bloom generator.
pub static SKIP_GLARE: AtomicBool = AtomicBool::new(false);
/// Skip the player-damage vignette.
pub static SKIP_PLAYER_DAMAGE: AtomicBool = AtomicBool::new(false);
/// Skip the sun-halo (PreApply + Apply).
pub static SKIP_SUN_HALO: AtomicBool = AtomicBool::new(false);
/// Skip the auto-exposure histogram (stalls adaptation; darkening bisection aid).
pub static SKIP_HISTOGRAM: AtomicBool = AtomicBool::new(false);

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
}

/// Read a stage's slot result texture (`CTX[slot+83]`, where `CTX` is the manager arg) and capture
/// it for the current eye into the debug overlay's per-stage preview.
fn capture_post_result(stage: usize, mgr: *mut c_void, slot: u32) {
    if mgr.is_null() {
        return;
    }
    let eye = crate::DRAW_INDEX.load(Ordering::Relaxed);
    unsafe {
        let result = (mgr as *const *mut jc3gi::graphics_engine::texture::Texture)
            .add(slot as usize + 83)
            .read();
        crate::capture_post_stage(stage, eye, result);
    }
}

#[detour(address = jc3gi::graphics_engine::post_effects::CMotionBlurEffect::Apply_ADDRESS)]
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
    crate::trace_eye(TraceEvent::MotionBlurApply {
        input,
        skip: SKIP_MOTION_BLUR.load(Ordering::Relaxed),
    });
    let slot = if SKIP_MOTION_BLUR.load(Ordering::Relaxed) {
        input
    } else {
        // `flag0` (a7) gates the reconstruction-filter motion blur (`if g_EnableMotionBlur && !a7`).
        // Forcing it true skips that reprojection blur -- the flicker -- but keeps the composite draw.
        let flag0 = flag0 || SKIP_MOTION_BLUR_RECON.load(Ordering::Relaxed);
        MOTION_BLUR_APPLY
            .get()
            .unwrap()
            .call(this, ctx, pec, mgr, input, blur, flag0, flag1)
    };
    capture_post_result(crate::POST_STAGE_MB, mgr, slot);
    slot
}

#[detour(address = jc3gi::graphics_engine::post_effects::CDepthOfFieldEffect::Apply_ADDRESS)]
fn dof_apply(
    this: *mut c_void,
    ctx: *mut c_void,
    pec: *mut PostEffectContext,
    mgr: *mut c_void,
    input: u32,
) -> u32 {
    crate::trace_eye(TraceEvent::DofApply {
        input,
        skip: SKIP_DOF.load(Ordering::Relaxed),
    });
    if SKIP_DOF.load(Ordering::Relaxed) {
        return input;
    }
    let slot = if DOF_NO_REPROJECT.load(Ordering::Relaxed) {
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

// CFadeEffect::Apply -- alpha-blended fade quad. Skip = no-op (the u64 return is discarded).
#[detour(address = jc3gi::graphics_engine::post_effects::CFadeEffect::Apply_ADDRESS)]
fn fade_apply(this: *mut c_void, a2: *mut c_void, a3: *mut c_void) -> u64 {
    crate::trace_eye(TraceEvent::FadeApply {
        skip: SKIP_FADE.load(Ordering::Relaxed),
    });
    if SKIP_FADE.load(Ordering::Relaxed) {
        return 0;
    }
    FADE_APPLY.get().unwrap().call(this, a2, a3)
}

// CGlareEffect::Apply -- bloom/glare generator. Skip = no bloom.
#[detour(address = jc3gi::graphics_engine::post_effects::CGlareEffect::Apply_ADDRESS)]
fn glare_apply(
    this: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
    a4: *mut c_void,
    a5: *mut c_void,
) -> u64 {
    crate::trace_eye(TraceEvent::GlareApply {
        skip: SKIP_GLARE.load(Ordering::Relaxed),
    });
    if SKIP_GLARE.load(Ordering::Relaxed) {
        return 0;
    }
    GLARE_APPLY.get().unwrap().call(this, a2, a3, a4, a5)
}

// CPlayerDamageEffect::Apply -- red damage vignette. Slot-passthrough; skip = return input slot.
#[detour(address = jc3gi::graphics_engine::post_effects::CPlayerDamageEffect::Apply_ADDRESS)]
fn player_damage_apply(
    this: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
    a4: *mut c_void,
    input: u32,
) -> u32 {
    crate::trace_eye(TraceEvent::PlayerDamageApply {
        input,
        skip: SKIP_PLAYER_DAMAGE.load(Ordering::Relaxed),
    });
    if SKIP_PLAYER_DAMAGE.load(Ordering::Relaxed) {
        return input;
    }
    PLAYER_DAMAGE_APPLY
        .get()
        .unwrap()
        .call(this, a2, a3, a4, input)
}

// CSunHaloEffect::PreApply -- prepares the halo. Skip = clear the ready flag (this+0x114) so the
// paired Apply early-outs, then no-op.
#[detour(address = jc3gi::graphics_engine::post_effects::CSunHaloEffect::PreApply_ADDRESS)]
fn sun_halo_pre_apply(this: *mut c_void, a2: *mut c_void, a3: *mut c_void, a4: *mut c_void) -> u64 {
    crate::trace_eye(TraceEvent::SunHaloPreApply {
        skip: SKIP_SUN_HALO.load(Ordering::Relaxed),
    });
    if SKIP_SUN_HALO.load(Ordering::Relaxed) {
        unsafe {
            *(this as *mut u8).add(0x114) = 0;
        }
        return 0;
    }
    SUN_HALO_PRE_APPLY.get().unwrap().call(this, a2, a3, a4)
}

// CSunHaloEffect::Apply -- composites the halo additively. Skip = no-op.
#[detour(address = jc3gi::graphics_engine::post_effects::CSunHaloEffect::Apply_ADDRESS)]
fn sun_halo_apply(this: *mut c_void, a2: *mut c_void) -> u64 {
    crate::trace_eye(TraceEvent::SunHaloApply {
        skip: SKIP_SUN_HALO.load(Ordering::Relaxed),
    });
    if SKIP_SUN_HALO.load(Ordering::Relaxed) {
        return 0;
    }
    SUN_HALO_APPLY.get().unwrap().call(this, a2)
}

// CToneMappingEffect::GenerateHistogramForFinalScene -- auto-exposure histogram. Skipping stalls
// adaptation, but preserve the out-param contract (this+764/765 = current slot indices) so callers
// don't read garbage. A bisection aid for the stereo darkening.
#[detour(
    address = jc3gi::graphics_engine::tone_mapping::CToneMappingEffect::GenerateHistogramForFinalScene_ADDRESS
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
    crate::trace_eye(TraceEvent::GenerateHistogram {
        skip: SKIP_HISTOGRAM.load(Ordering::Relaxed),
    });
    // The histogram reads the final HDR scene (MainColor) for auto-exposure, so this is the first
    // point in the post chain where MainColor still holds this dispatch's clean scene -- grab it for
    // the per-eye "Scene" preview before the chain recycles it.
    crate::capture_main_color(crate::DRAW_INDEX.load(Ordering::Relaxed));

    if SKIP_HISTOGRAM.load(Ordering::Relaxed) {
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
