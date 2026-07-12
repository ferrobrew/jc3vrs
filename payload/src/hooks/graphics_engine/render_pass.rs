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
    graphics_engine::{HContext_t, RenderContext},
    render_engine::{RenderEngine, RenderPassId},
    render_pass::{RenderPass, RenderPassState},
    shadow_manager::ShadowManager,
};
use re_utilities::hook_library::HookLibrary;
use windows::{
    Win32::{
        Graphics::Direct3D11::{D3D11_TEXTURE2D_DESC, ID3D11Resource, ID3D11Texture2D},
        System::Threading::{EnterCriticalSection, LeaveCriticalSection},
    },
    core::Interface as _,
};

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
        .with_static_binder(&PRE_DRAW_BINDER)
        .with_static_binder(&DRAW_POSTEFFECTS_BINDER)
        .with_static_binder(&SETUP_RENDER_STATES_BINDER)
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

    // Draw maximal runs of non-skipped passes in [lo, hi), omitting the skipped ones.
    let draw = |lo: i32, hi: i32| {
        let mut run_start = lo;
        for pass in lo..hi {
            if skipped(pass) {
                if run_start < pass {
                    original.call(this, ctx, setup, run_start, pass);
                }
                run_start = pass + 1;
            }
        }
        if run_start < hi {
            original.call(this, ctx, setup, run_start, hi);
        }
    };

    // Foveation (issue #29): write the peripheral stencil mask just before the foveated shading sub-range,
    // then force the peripheral stencil test through it so the GPU skips the dropped GBuffer pixels. The
    // reconstruction fill-in runs later, in the `DrawPosteffects` hook, once the scene is fully lit --
    // filling here (right after the GBuffer passes) would reconstruct an as-yet-unlit MainColor. `plan` is
    // `None` when foveation is off or this is not a VR eye render, collapsing to the plain split above.
    let Some(plan) = foveation_plan(first, last) else {
        draw(first, last);
        return;
    };
    draw(first, plan.fov_start);
    if plan.do_mask {
        run_foveation_pass(crate::vr::foveation::mask_write, plan.params, "mask-write");
    }
    crate::vr::foveation::FORCE_STENCIL_TEST.store(true, std::sync::atomic::Ordering::Relaxed);
    draw(plan.fov_start, plan.fov_end);
    crate::vr::foveation::FORCE_STENCIL_TEST.store(false, std::sync::atomic::Ordering::Relaxed);
    draw(plan.fov_end, last);
}

/// The foveation bracketing for one [`draw_render_pass_range`] call: the clamped foveated sub-range
/// `[fov_start, fov_end)`, whether this call owns the mask-write / fill-in (so each runs once even if the
/// scene range is split across calls), and the per-eye pass parameters.
struct FoveationPlan {
    fov_start: i32,
    fov_end: i32,
    do_mask: bool,
    params: crate::vr::foveation::FoveationParams,
}

/// Build the [`FoveationPlan`] for a draw-range call, or `None` when foveation is disabled, this is not a
/// VR eye render, or the foveated range does not intersect `[first, last)`. Publishes the per-draw stencil
/// test bits ([`crate::vr::foveation::STENCIL_TEST_BITS`]) when active.
fn foveation_plan(first: i32, last: i32) -> Option<FoveationPlan> {
    let cfg = Config::lock_query(|c| c.foveation.clone());
    if !cfg.enabled {
        return None;
    }
    let eye = usize::from(is_second_eye());
    let center_uv = foveal_center_uv(eye)?;
    let ff = cfg.foveal_first_pass as i32;
    let fl = cfg.foveal_last_pass as i32;
    let fov_start = first.max(ff);
    let fov_end = last.min(fl + 1);
    if fov_start >= fov_end {
        return None;
    }
    crate::vr::foveation::STENCIL_TEST_BITS.store(
        crate::vr::foveation::packed_stencil_test(cfg.mask_bit),
        std::sync::atomic::Ordering::Relaxed,
    );
    Some(FoveationPlan {
        fov_start,
        fov_end,
        do_mask: (first..last).contains(&ff),
        params: foveation_params(&cfg, center_uv),
    })
}

/// Build the pass parameters from the config and the eye's foveal centre.
fn foveation_params(
    cfg: &crate::config::FoveationConfig,
    center_uv: [f32; 2],
) -> crate::vr::foveation::FoveationParams {
    crate::vr::foveation::FoveationParams {
        center_uv,
        inner_fraction: cfg.inner_fraction,
        outer_fraction: cfg.outer_fraction,
        max_drop: cfg.max_drop,
        mask_bit: cfg.mask_bit,
        debug_show_mask: cfg.debug_show_mask,
    }
}

/// The per-eye foveal centre as a UV, from the eye's projection principal point (`m02`/`m12` shear terms
/// of the column-major projection): symmetric frustums map to the buffer centre, canted HMDs to their
/// off-axis centre. `None` when no VR frame is in flight. Clamped to the buffer.
fn foveal_center_uv(eye: usize) -> Option<[f32; 2]> {
    let m = crate::vr::render_params(eye)?.projection_standard;
    Some([
        (0.5 - 0.5 * m[8]).clamp(0.0, 1.0),
        (0.5 + 0.5 * m[9]).clamp(0.0, 1.0),
    ])
}

/// Run one foveation D3D pass, warning on any failure without disturbing the surrounding scene draw.
fn run_foveation_pass(
    pass: fn(crate::vr::foveation::FoveationParams) -> anyhow::Result<()>,
    params: crate::vr::foveation::FoveationParams,
    label: &str,
) {
    if let Err(e) = pass(params) {
        tracing::warn!(target: "vr", "foveation {label} pass failed: {e:#}");
    }
}

// RenderEngine::DrawPosteffects -- the post-effects pass, drawn once per eye after the scene is fully
// composed (opaque + lighting + sky + transparency resolved into MainColor) and before post-processing
// reads it. Foveation's reconstruction fill-in runs here (issue #29): by now the peripheral pixels the
// mask/force-test dropped in the GBuffer passes have been lit black, so the fill reconstructs each from
// its kept neighbours in the finished MainColor. Runs for both eyes; a no-op when foveation is off.
#[detour(address = jc3gi::graphics_engine::render_engine::RenderEngine::DrawPosteffects_ADDRESS)]
fn draw_posteffects(this: *mut c_void, ctx: *mut c_void, setup: *mut c_void) {
    if let Some(cfg) = Config::lock_query(|c| c.foveation.enabled.then(|| c.foveation.clone())) {
        let eye = usize::from(is_second_eye());
        if let Some(center_uv) = foveal_center_uv(eye) {
            run_foveation_pass(
                crate::vr::foveation::fill_in,
                foveation_params(&cfg, center_uv),
                "fill-in",
            );
        }
    }
    DRAW_POSTEFFECTS.get().unwrap().call(this, ctx, setup);
}

// Graphics::SetupRenderStates -- the pre-draw pipeline-state flush that decodes each dirty packed state
// index on the context and issues the underlying D3D `OMSet*State` binds. While foveation forces the
// peripheral stencil test (issue #29), rewrite the staged depth-stencil index of each non-stencil draw to
// add an `EQUAL 0` test against the mask bit, so the GPU skips the pixels the mask pass tagged. Patched
// before the original flushes; a no-op unless foveation is mid-range (see `crate::vr::foveation`).
#[detour(address = jc3gi::graphics_engine::graphics_engine::SetupRenderStates_ADDRESS)]
fn setup_render_states(context: *mut HContext_t, a2: *mut c_void) {
    // SAFETY: `context` is the live render context the game passed to its own state flush.
    unsafe { crate::vr::foveation::apply_force_test(context) };
    SETUP_RENDER_STATES.get().unwrap().call(context, a2);
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
//   * stabilize_shadow_fit: horizontalize the active camera's forward vector (m_TransformT1 row 2,
//     data[8..10]) to yaw-only for the fit. The engine pushes each cascade box's centre forward along
//     that vector, so head pitch/roll slides the cascade centre and the shadows shift, re-quantize, and
//     scale as you look around (view-dependent shadows). Projecting the forward onto the ground plane
//     makes the centre follow heading but not head tilt; restored after so rendering is unaffected. The
//     box size (sphere) and orientation (sun-fixed) are already view-independent.
#[detour(
    address = jc3gi::graphics_engine::shadow_manager::ShadowManager::UpdateRender_ADDRESS
)]
fn shadow_manager_update_render(this: *mut c_void, dt: f32, dtf: f32) -> u64 {
    let original = SHADOW_MANAGER_UPDATE_RENDER.get().unwrap();
    let (unjitter, widen, stabilize, update_every_frame) = Config::lock_query(|c| {
        (
            c.stereo.unjitter_shadow_fit,
            c.stereo.widen_shadow_fit,
            c.stereo.stabilize_shadow_fit,
            c.stereo.shadow_update_every_frame,
        )
    });
    // Defeat the cascade update-pattern amortization (issue #31): force every cascade to update level 0
    // so it re-fits and re-renders each frame instead of on its 2^L cadence, eliminating the periodic
    // re-fit snaps that surface as flicker with the mod's forced SMAA 1x (no T2X to average them). The
    // levels persist frame to frame and are read by `SetActiveShadowPassCount` (which runs before this
    // in the sim's shadow update), so zeroing them here holds for the next frame's schedule build.
    if update_every_frame && let Some(manager) = unsafe { this.cast::<ShadowManager>().as_mut() } {
        manager.m_CascadeUpdateLevels = [0; 6];
    }
    // The widen reuses the union-FOV cull projection; `None` on flatscreen, so widening is VR-only.
    let union = widen.then(crate::vr::cull_projection_standard).flatten();
    if !unjitter && union.is_none() && !stabilize {
        return original.call(this, dt, dtf);
    }
    // SAFETY: the camera-manager singleton and the active camera are live on the game thread for
    // the duration of this call; the projection and transform writes are scoped save/restore.
    unsafe {
        let camera = jc3gi::camera::camera_manager::CameraManager::get()
            .map(|cm| cm.m_ActiveCamera)
            .unwrap_or(std::ptr::null_mut());
        let Some(camera) = camera.as_mut() else {
            return original.call(this, dt, dtf);
        };
        let saved_proj = [
            camera.m_ProjectionF.data[0],
            camera.m_ProjectionF.data[5],
            camera.m_ProjectionF.data[12],
            camera.m_ProjectionF.data[13],
        ];
        if unjitter {
            camera.m_ProjectionF.data[12] = 0.0;
            camera.m_ProjectionF.data[13] = 0.0;
        }
        if let Some(union) = union {
            camera.m_ProjectionF.data[0] = union[0];
            camera.m_ProjectionF.data[5] = union[5];
        }
        // Horizontalize the camera forward (row 2) so the cascade centre's forward-push is yaw-only.
        let saved_row2 = stabilize.then(|| {
            let row2 = [
                camera.m_TransformT1.data[8],
                camera.m_TransformT1.data[9],
                camera.m_TransformT1.data[10],
            ];
            let len = (row2[0] * row2[0] + row2[2] * row2[2]).sqrt();
            if len > 1e-4 {
                camera.m_TransformT1.data[8] = row2[0] / len;
                camera.m_TransformT1.data[9] = 0.0;
                camera.m_TransformT1.data[10] = row2[2] / len;
            }
            row2
        });
        let result = original.call(this, dt, dtf);
        camera.m_ProjectionF.data[0] = saved_proj[0];
        camera.m_ProjectionF.data[5] = saved_proj[1];
        camera.m_ProjectionF.data[12] = saved_proj[2];
        camera.m_ProjectionF.data[13] = saved_proj[3];
        if let Some(row2) = saved_row2 {
            camera.m_TransformT1.data[8] = row2[0];
            camera.m_TransformT1.data[9] = row2[1];
            camera.m_TransformT1.data[10] = row2[2];
        }
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

// RenderEngine::PreDraw -- the pre-pass dispatch (sky-lighting LUT, planar/env reflections, cloud
// shadows, vegetation, the sun-shadow cascade atlas, water sim, rain occluder). Most of these are driven
// by the sun / reflection / world-space cameras -- never the per-eye render camera -- and write separate
// persistent render targets the per-eye passes never overwrite, so their output is identical between the
// two eyes. On the SECOND eye, skip the view-independent categories (clear their m_Enabled so PreDraw's
// loop no-ops them) and reuse eye 0's output: this halves the second eye's pre-pass cost (shadow-map
// render, reflection proxies, water sim, cloud shadows), and renders the shared sun-shadow atlas once per
// frame instead of twice -- which also removes the per-eye shadow flicker (issue #31). PreDraw runs after
// CommitRenderPassSettings (which re-enables the frame's scheduled shadow cascades), so the clear here is
// the last word before the loop; the passes are re-enabled after so the next frame's first eye runs them.
// Gated on `restore_frame_counters` so both eyes share the shadow-atlas parity slot (without it, eye 1
// advances parity and would sample the other, unrendered slot).
#[detour(address = jc3gi::graphics_engine::render_engine::RenderEngine::PreDraw_ADDRESS)]
fn pre_draw(this: *mut RenderEngine, ctx: *mut HContext_t) -> u64 {
    let original = PRE_DRAW.get().unwrap();
    let (share_cfg, sync) = Config::lock_query(|c| {
        (
            c.stereo.share_prepasses && c.stereo.restore_frame_counters,
            c.stereo.sync_shadow_atlas,
        )
    });
    let share = is_second_eye() && share_cfg;
    let result = if share {
        // SAFETY: `this` is the live render engine; its per-category `m_RenderPasses` vectors hold
        // engine-owned, null-checked pass pointers; the `m_Enabled` flag write mirrors the shadow
        // scheduler's own store in `commit_render_pass_settings`.
        let disabled = unsafe { disable_shared_prepasses(this) };
        let r = original.call(this, ctx);
        unsafe { reenable_passes(&disabled) };
        r
    } else {
        original.call(this, ctx)
    };
    // Sync the shadow-atlas parity after it was (re)rendered this dispatch (issue #31). When the
    // shared-prepass skip disabled the atlas passes on eye 1, the atlas was not re-rendered -- its two
    // halves are already in sync from eye 0 -- so skip the copy there.
    if sync && !share {
        sync_shadow_atlas();
    }
    result
}

/// Sync the sun-shadow atlas parity double buffer (issue #31). The engine renders the cascade atlas into
/// the current frame parity's half of its `Texture2DArray` and the material shaders sample the same-parity
/// half; the parity flips every frame, so consecutive frames sample slices rendered at slightly different
/// head poses and the whole scene's brightness alternates a few percent -- the flicker. After the atlas
/// renders, copy the freshly-rendered half onto the other half so both hold identical content; whichever
/// half the shader then samples, the shadow is the same, and nothing alternates. A GPU copy on the render
/// thread under the engine context mutex -- it mutates no shared CPU counter (the reason the earlier
/// frame-counter parity pin raced the engine and crashed).
fn sync_shadow_atlas() {
    // SAFETY: called from `PreDraw` on the render thread after the atlas rendered; the engine device,
    // context, and shadow manager are live, and only their inline COM handles are borrowed for the copy.
    unsafe {
        let Some(ge) = jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() else {
            return;
        };
        let Some(mgr) = ge.m_ShadowManager.as_ref() else {
            return;
        };
        let Some(atlas) = mgr.m_AtlasTexture.as_ref() else {
            return;
        };
        let Some(device) = ge.m_Device.as_ref() else {
            return;
        };
        let Some(context) = device.m_Context.as_ref() else {
            return;
        };
        // The atlas's inline `ID3D11Resource` handle, read as a raw pointer so a null slot never
        // materializes a non-null-invariant `windows` interface.
        let raw = *(std::ptr::addr_of!(atlas.m_Texture) as *const *mut c_void);
        let Some(res) = ID3D11Resource::from_raw_borrowed(&raw) else {
            return;
        };
        let Ok(tex2d) = res.cast::<ID3D11Texture2D>() else {
            return;
        };
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        tex2d.GetDesc(&mut desc);
        if desc.ArraySize < 2 {
            return;
        }
        // The two parity halves span the array; copy the rendered parity's slices onto the other's.
        let half = desc.ArraySize / 2;
        let mips = desc.MipLevels.max(1);
        let parity = (jc3gi::graphics_engine::graphics_engine::get_render_frame_counters()
            .m_FrameIndex
            & 1) as usize;
        let src_base = mgr.m_SliceBase[parity];
        let dst_base = mgr.m_SliceBase[parity ^ 1];
        let ctx = &context.m_Context;
        EnterCriticalSection(context.m_Mutex);
        for i in 0..half {
            // Guard against an unexpected slice base producing an out-of-range subresource.
            if src_base + i >= desc.ArraySize || dst_base + i >= desc.ArraySize {
                continue;
            }
            ctx.CopySubresourceRegion(
                res,
                (dst_base + i) * mips,
                0,
                0,
                0,
                res,
                (src_base + i) * mips,
                None,
            );
        }
        LeaveCriticalSection(context.m_Mutex);
    }
}

/// The pre-pass categories ([`RenderPassId`] indices) that render identically for both eyes and whose
/// outputs persist for the whole frame, so eye 1 can reuse eye 0's: planar + environment reflections
/// (`9..=17`), cloud shadows (`18`), the static/dynamic/reflective sun-shadow cascade atlas (`22..=40`,
/// which also fixes the per-eye shadow flicker #31), and the water-simulation compute (`41..=44`).
/// Terrain-patch prep (`1..=7`, per-eye), the sky-lighting LUT (`8`), vegetation (`19..=21`), and the
/// rain occluder (`45`) are held per-eye for this conservative first cut.
const SHARED_PREPASS_CATEGORIES: &[(usize, usize)] = &[(9, 18), (22, 44)];

/// Clear [`RenderPassState::m_Enabled`] on every enabled pass in the shared pre-pass categories so
/// `PreDraw`'s loop skips them, returning the passes cleared so [`reenable_passes`] can restore them.
///
/// # Safety
///
/// `this` is the live render engine during its own `PreDraw`; the passes are engine-owned.
unsafe fn disable_shared_prepasses(this: *mut RenderEngine) -> Vec<*mut RenderPass> {
    let Some(engine) = (unsafe { this.as_mut() }) else {
        return Vec::new();
    };
    let mut disabled = Vec::new();
    for &(lo, hi) in SHARED_PREPASS_CATEGORIES {
        for cat in lo..=hi {
            for &pass in unsafe { engine.m_RenderPasses[cat].as_slice() } {
                if let Some(pass) = (unsafe { pass.as_mut() })
                    && pass.m_StateFlags.contains(RenderPassState::m_Enabled)
                {
                    pass.m_StateFlags.remove(RenderPassState::m_Enabled);
                    disabled.push(pass as *mut RenderPass);
                }
            }
        }
    }
    disabled
}

/// Re-enable the pre-passes disabled by [`disable_shared_prepasses`], so the next frame's first eye
/// renders them.
///
/// # Safety
///
/// The pointers came from the live `m_RenderPasses` vectors this same frame and are still valid.
unsafe fn reenable_passes(passes: &[*mut RenderPass]) {
    for &pass in passes {
        if let Some(pass) = unsafe { pass.as_mut() } {
            pass.m_StateFlags.insert(RenderPassState::m_Enabled);
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
    // SAFETY: the graphics engine and its shadow manager are live on the render thread during a dispatch.
    let shadow_fade = unsafe {
        jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get()
            .and_then(|ge| ge.m_ShadowManager.as_ref())
            .map_or(1.0, |mgr| mgr.GetShadowFade())
    };
    let t = &ctx.m_ShadowCascades.m_Transform.data;
    TraceState::record_eye(TraceEvent::ShadowState {
        counter: counters.m_Counter,
        frame_index: counters.m_FrameIndex,
        translation: [t[12], t[13], t[14], t[15]],
        scale_blend: std::array::from_fn(|i| ctx.m_ShadowCascades.m_ScaleBlend[i].data),
        offset_radius: std::array::from_fn(|i| ctx.m_ShadowCascades.m_OffsetRadius[i].data),
        active_cascades: u32::from(ctx.m_ActiveCascadeCount),
        anchor_delta,
        shadow_fade,
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
