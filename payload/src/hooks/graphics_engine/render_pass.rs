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
use jc3gi::graphics_engine::{
    graphics_engine::RenderContext, render_engine::RenderPassId, render_pass::RenderPassState,
    shadow_manager::ShadowManager,
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    debug::trace::{TraceEvent, TraceState},
    stereo::is_second_eye,
};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&SETUP_RENDER_FRAME_DATA_BINDER)
        .with_static_binder(&HAND_BACK_BUFFERS_BINDER)
        .with_static_binder(&DRAW_RENDER_PASS_RANGE_BINDER)
        .with_static_binder(&SET_GLOBAL_SHADER_CONSTANTS_BINDER)
        .with_static_binder(&COMMIT_RENDER_PASS_SETTINGS_BINDER)
        .with_static_binder(&SHADOW_MANAGER_UPDATE_RENDER_BINDER)
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

const RP_AO_VOLUMES: i32 = RenderPassId::RP_AO_VOLUMES as i32;
const RP_SCREEN_SPACE_REFLECTIONS: i32 = RenderPassId::RP_SCREEN_SPACE_REFLECTIONS as i32;
const RP_GLOBAL_ILLUMINATION: i32 = RenderPassId::RP_GLOBAL_ILLUMINATION as i32;

// RenderEngine::DrawRenderPassRange -- draws the half-open pass-index range [first, last). The
// per-eye-divergence and flicker diagnostics drop passes by splitting the range around them, so
// every other pass runs untouched: SSR (reads a previous-frame scene capture regenerated each Draw)
// and GI (may carry a per-eye temporal/probe history) for the per-eye MainColor divergence, AO
// volumes (depth-tested proxy geometry whose whole contribution can flip on a sub-pixel jitter
// shift) for the blob-scale shadow flicker, and an arbitrary range for bisecting whichever pass an
// artifact lives in.
#[detour(address = jc3gi::graphics_engine::render_engine::RenderEngine::DrawRenderPassRange_ADDRESS)]
fn draw_render_pass_range(
    this: *mut c_void,
    ctx: *mut c_void,
    setup: *mut c_void,
    first: i32,
    last: i32,
) {
    let original = DRAW_RENDER_PASS_RANGE.get().unwrap();
    let (skip_ssr, skip_gi, skip_ao_volumes, skip_range) = Config::lock_query(|c| {
        (
            c.stereo.skip_ssr,
            c.stereo.skip_gi,
            c.stereo.skip_ao_volumes,
            c.stereo
                .skip_pass_range_enabled
                .then_some(c.stereo.skip_pass_range),
        )
    });

    let skipped = |pass: i32| {
        (skip_ssr && pass == RP_SCREEN_SPACE_REFLECTIONS)
            || (skip_gi && pass == RP_GLOBAL_ILLUMINATION)
            || (skip_ao_volumes && pass == RP_AO_VOLUMES)
            || skip_range.is_some_and(|(lo, hi)| lo <= pass && pass <= hi)
    };

    // Draw maximal runs of non-skipped passes, omitting the skipped ones.
    let mut lo = first;
    for pass in first..last {
        if skipped(pass) {
            if lo < pass {
                original.call(this, ctx, setup, lo, pass);
            }
            lo = pass + 1;
        }
    }
    if lo < last {
        original.call(this, ctx, setup, lo, last);
    }
}

// ShadowManager::UpdateRender -- the sim-side sun-shadow update, which fits the scheduled cascades to
// the active camera (the fit frustum comes from its m_ProjectionF via CFrustum::Compute). Two scoped
// projection tweaks around the fit, both restored after:
//   * unjitter_shadow_fit: strip the projection's clip-space jitter translation (data[12]/[13]) so a
//     jittered fit frustum can't re-quantize the cascade texel snap mid-transition. The active sim
//     camera is not jittered by the mod, so this showed no effect on issue #10; kept as a defensive
//     A/B.
//   * widen_shadow_fit: widen the two FOV-scale terms (data[0]/data[5]) to the union FOV so the
//     cascades are fit to cover BOTH eyes. The fit is once-per-frame from the narrow centre camera, so
//     the wider, laterally shifted VR eyes otherwise exceed the fitted coverage box -- their distant
//     shadows fall outside it and disagree between the eyes, and the boundary crawls under motion.
//     This is the coverage half of the shadow fix; fix_shadow_cascade_anchor is the sampling half. The
//     centre, shear, and z (near/far/split) terms are left untouched, so split distances are unchanged.
#[detour(
    address = jc3gi::graphics_engine::shadow_manager::ShadowManager::UpdateRender_ADDRESS
)]
fn shadow_manager_update_render(this: *mut c_void, dt: f32, dtf: f32) -> u64 {
    let original = SHADOW_MANAGER_UPDATE_RENDER.get().unwrap();
    let (unjitter, widen) =
        Config::lock_query(|c| (c.stereo.unjitter_shadow_fit, c.stereo.widen_shadow_fit));
    // The widen reuses the union-FOV cull projection; `None` on flatscreen, so widening is VR-only.
    let union = widen.then(crate::vr::cull_projection_standard).flatten();
    if !unjitter && union.is_none() {
        return original.call(this, dt, dtf);
    }
    // SAFETY: the camera-manager singleton and the active camera are live on the game thread for
    // the duration of this call; the projection writes are scoped save/restore.
    unsafe {
        let camera = jc3gi::camera::camera_manager::CameraManager::get()
            .map(|cm| cm.m_ActiveCamera)
            .unwrap_or(std::ptr::null_mut());
        let Some(camera) = camera.as_mut() else {
            return original.call(this, dt, dtf);
        };
        let data = &mut camera.m_ProjectionF.data;
        let saved = [data[0], data[5], data[12], data[13]];
        if unjitter {
            data[12] = 0.0;
            data[13] = 0.0;
        }
        if let Some(union) = union {
            data[0] = union[0];
            data[5] = union[5];
        }
        let result = original.call(this, dt, dtf);
        data[0] = saved[0];
        data[5] = saved[1];
        data[12] = saved[2];
        data[13] = saved[3];
        result
    }
}

// ShadowManager::CommitRenderPassSettings -- the per-dispatch gate that enables this frame's
// scheduled shadow passes (the update round-robin) and re-points their targets by parity. With the
// freeze diagnostic on, the pass-enable flags the original just set are cleared again (mirroring its
// own prologue), so no shadow pass renders and the atlas keeps its last contents -- shadows stay
// visible but stop updating, splitting "atlas content pulses" from "shadow sampling pulses".
#[detour(
    address = jc3gi::graphics_engine::shadow_manager::ShadowManager::CommitRenderPassSettings_ADDRESS
)]
fn commit_render_pass_settings(this: *mut ShadowManager, ctx: *mut c_void) {
    COMMIT_RENDER_PASS_SETTINGS.get().unwrap().call(this, ctx);
    if !Config::lock_query(|c| c.stereo.freeze_shadow_maps) {
        return;
    }
    // SAFETY: `this` is the live shadow manager; each cascade's pass pointers are engine-owned and
    // null-checked, and the flag write mirrors the original's own prologue stores.
    unsafe {
        let Some(manager) = this.as_mut() else {
            return;
        };
        for cascade in &mut manager.m_Cascades {
            for pass in cascade.m_Passes {
                if let Some(pass) = pass.as_mut() {
                    pass.m_StateFlags.remove(RenderPassState::m_Enabled);
                }
            }
        }
    }
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
    if let Some(ctx) = unsafe { ctx.cast::<RenderContext>().as_mut() } {
        let delta = crate::stereo::STEREO_STATE.lock().shadow_anchor_delta;
        record_shadow_state(ctx, delta);
        if Config::lock_query(|c| c.stereo.fix_shadow_cascade_anchor) {
            apply_shadow_cascade_anchor_fix(ctx, delta);
        }
    }
    SET_GLOBAL_SHADER_CONSTANTS.get().unwrap().call(this, ctx);
}

/// Record the staged sun-shadow constants into the active render trace (no-op outside a trace) --
/// the raw parity-slot values, read before the anchor correction mutates them. See
/// [`TraceEvent::ShadowState`] for how the series is analysed.
fn record_shadow_state(ctx: &RenderContext, anchor_delta: [f32; 3]) {
    if !crate::debug::trace::tracing_active() {
        return;
    }
    // SAFETY: the render-frame counters live for the process.
    let counters = unsafe { *jc3gi::graphics_engine::graphics_engine::get_render_frame_counters() };
    let t = &ctx.m_ShadowCascades.m_Transform.data;
    TraceState::record_eye(TraceEvent::ShadowState {
        counter: counters.m_Counter,
        frame_index: counters.m_FrameIndex,
        translation: [t[12], t[13], t[14], t[15]],
        scale_blend: std::array::from_fn(|i| ctx.m_ShadowCascades.m_ScaleBlend[i].data),
        offset_radius: std::array::from_fn(|i| ctx.m_ShadowCascades.m_OffsetRadius[i].data),
        active_cascades: u32::from(ctx.m_ActiveCascadeCount),
        anchor_delta,
    });
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
