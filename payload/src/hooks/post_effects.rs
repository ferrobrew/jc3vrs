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
use jc3gi::graphics_engine::post_effects::PostEffectContext;
use re_utilities::hook_library::HookLibrary;

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

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&MOTION_BLUR_APPLY_BINDER)
        .with_static_binder(&DOF_APPLY_BINDER)
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
    if SKIP_MOTION_BLUR.load(Ordering::Relaxed) {
        return input;
    }
    // `flag0` (a7) gates the reconstruction-filter motion blur (`if g_EnableMotionBlur && !a7`).
    // Forcing it true skips that reprojection blur -- the flicker -- but keeps the composite draw.
    let flag0 = flag0 || SKIP_MOTION_BLUR_RECON.load(Ordering::Relaxed);
    MOTION_BLUR_APPLY
        .get()
        .unwrap()
        .call(this, ctx, pec, mgr, input, blur, flag0, flag1)
}

#[detour(address = jc3gi::graphics_engine::post_effects::CDepthOfFieldEffect::Apply_ADDRESS)]
fn dof_apply(
    this: *mut c_void,
    ctx: *mut c_void,
    pec: *mut PostEffectContext,
    mgr: *mut c_void,
    input: u32,
) -> u32 {
    if SKIP_DOF.load(Ordering::Relaxed) {
        return input;
    }
    if DOF_NO_REPROJECT.load(Ordering::Relaxed) {
        // DoF's motion-vector reprojection (the flicker) is gated by bit 0 of the render context's
        // flags, inside its full composite/grade branch. Clear just that bit for the call so DoF
        // still grades the scene but skips the reprojection, then restore.
        unsafe {
            if let Some(rc) = pec.as_mut().and_then(|p| p.m_RenderContext.as_mut()) {
                let saved = rc.m_Flags;
                rc.m_Flags = saved & !1;
                let result = DOF_APPLY.get().unwrap().call(this, ctx, pec, mgr, input);
                rc.m_Flags = saved;
                return result;
            }
        }
    }
    DOF_APPLY.get().unwrap().call(this, ctx, pec, mgr, input)
}
