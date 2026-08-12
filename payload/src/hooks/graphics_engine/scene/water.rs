//! The legacy (non-WaveWorks) water render blocks' screen-space reflection/refraction lookup, biased
//! into each eye's half of the double-wide target under the single-pass collapse.
//!
//! These blocks sample `ReflectionMap`, `RefractionMap`, and `DepthMap` through a *projective*
//! coordinate rather than `SV_Position`: their block type stages a world→screen-UV matrix on vertex
//! `cb1` once per pass, the vertex shader transforms the water vertex by it and passes the result on
//! as `TEXCOORD1`, and the pixel shader divides by `w`. The NDC→UV half-scale is already folded into
//! the CPU-side matrix, so the UV it yields is normalized over the **viewport** -- one eye's half --
//! while every buffer it indexes is the whole double-wide target. Each eye therefore reads the entire
//! two-eye image stretched across its water surface, and because the error is a fixed 2x scale it is a
//! 2x motion gain too: the reflections slide over the water as the camera moves.
//!
//! The fix is four rows of arithmetic on the matrix the type already staged --
//! `u' = (u + eye) · 0.5` -- applied around a per-eye re-issue of the block's `Draw`
//! ([`screen_uv_cb_per_eye`](crate::stereo::single_pass::screen_uv_cb_per_eye)), which is also what
//! makes the eye known. No shader is touched: the water vertex shaders take their clip position from
//! the global `cb0`, which the collapse already handles, and the projective coordinate is entirely a
//! CPU-side constant.
//!
//! The WaveWorks family (`NvWater*`, [`NvWaterHighEndRenderBlock`]) does not have that defect -- its
//! shaders build the screen UV as `SV_Position × (1/2W, 1/H)`, which is already self-consistent under
//! double-wide -- but it has the other one, and the second half of this module fixes it: the whole
//! family takes its clip position from a model-view-projection the block bakes into its own constant
//! buffer, so the collapse's per-eye machinery never reaches it and both eyes see the collapsed centre
//! view of the water surface. See [`nv_water_per_eye`].
//!
//! Independent of the collapse, [`wave_works_simulation_step`] also holds the WaveWorks simulation
//! to one step per real frame across the two-pass stereo dispatches (issue #47): the step lives
//! inside the main-body water draw, so a per-eye dispatch would otherwise advance the ocean between
//! the eyes and decorrelate the sun glint.
//!
//! Not covered: the water-box *surface* geometry (`WaterBoxRenderBlock::DrawSurface`, and the
//! `NWater::DrawWaterBoxSurface` loop [`NvWaterHighEndRenderBlock::Draw`] runs over every registered
//! box). Neither of the two mechanisms above reaches it, and neither is its defect. Its vertex shader
//! (`waterboxsurface`) builds clip as
//!
//! ```text
//! world_rel = box_transform(cb1[0..3]) · position     // scale by half-extents + (centre - camera)
//! clip      = cb0[0..3] · (world_rel + cb0[4])        // full view-projection · absolute world
//! ```
//!
//! -- the *full*, translation-bearing view-projection at global rows `0..3`, which the collapse's
//! per-eye register remap does not cover (it claims only `cb0[4]` and `cb0[29..32]`). The remap does
//! claim the shader, on that lone `cb0[4]` camera-position reference, and retargets it to `cb13`
//! while leaving the projection centred -- so the eye offset is added to the *world position* and
//! then viewed from the centre, which displaces the surface by the eye offset in the wrong direction
//! instead of giving it parallax. The per-eye re-issue below draws with one instance, so the parity
//! resolves to eye 0 in both halves and both eyes get eye 0's displacement.
//!
//! This is the whole legacy family's idiom, not one permutation's: `waterbox`, `waterboxbelow`,
//! `watershader_lod0`, and `watershader_lod1` read the same rows. The transform they want is the
//! reprojection rewrite, which replaces the clip position wholesale and so does not care that the
//! source was `cb0[0..3]` -- but reprojecting them moves where they rasterize, which invalidates the
//! projective screen UV the first half of this module corrects (that fix is deliberately *not*
//! reprojected, precisely because the geometry still lands at the centre view). The two are one
//! change, and the surface grid additionally has no per-eye re-issue of its own to hang it off.
//! See `docs/mod/stereo/single-pass-stereo.md`.

use std::{
    ffi::c_void,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering},
};

use detours_macro::detour;
use jc3gi::{
    graphics_engine::{
        graphics_engine::RenderContext,
        render_block::{
            NvWaterHighEndRenderBlock, NvWaterHighEndRenderBlockType, RBIInfo, WaterBoxRenderBlock,
            WaterHighEndRenderBlock,
        },
        render_engine::RenderPassId,
    },
    types::math::Matrix4,
    water_patch_manager::WaterPatchManager,
};
use parking_lot::Mutex;
use re_utilities::hook_library::HookLibrary;
use serde::Serialize;

use crate::config::Config;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&WATER_HIGH_END_DRAW_BINDER)
        .with_static_binder(&WATER_BOX_DRAW_BINDER)
        .with_static_binder(&NV_WATER_HIGH_END_DRAW_BINDER)
        .with_static_binder(&WAVE_WORKS_SIMULATION_STEP_BINDER)
}

/// Reset the once-per-frame simulation latch at the frame's first dispatch. Called from the
/// `PreDraw` dispatch prologue on the draw thread -- the same seam
/// [`crate::stereo::single_pass::begin_dispatch`] uses, and for the same reason: with the frame
/// tail deferred, the game thread runs concurrently with the previous frame's still-walking
/// dispatch, so a game-thread frame-start reset could re-arm the step under that dispatch's live
/// water draw.
pub(crate) fn begin_dispatch() {
    if crate::stereo::dispatch_ordinal() == 0 {
        SIMULATION_STEPPED_THIS_FRAME.store(false, Ordering::Relaxed);
        WATER_FRAME.fetch_add(1, Ordering::Relaxed);
    }
}

/// Everything the WaveWorks water draw read for one dispatch, snapshotted at the main-body `Draw`
/// for the F12 sidecar (issue #47): the per-eye water mismatch survives every input we can reason
/// about statically, so each capture records what the two eyes' draws were *actually* given --
/// texture identities as pointers (equal pointers between the eyes mean shared content), the
/// shader-permutation selectors, and the simulation clock.
#[derive(Clone, Serialize)]
pub struct WaterDrawSnapshot {
    /// The value of [`WATER_FRAME`] at the draw, so the sidecar shows whether the two eyes'
    /// snapshots come from the same frame.
    pub frame: u64,
    pub dispatch_ordinal: usize,
    /// The pass the draw ran under (`RenderPassId`); `RP_WATER` (109) is the expected scene pass.
    pub render_pass: i32,
    /// Selects the below-water shader permutation and tweak set.
    pub under_water: bool,
    /// Whether the ocean simulation is altitude-paused this draw.
    pub altitude_simulation_pause: bool,
    /// The block's simulation clock in seconds.
    pub render_time: f64,
    /// `WaterPatchManager::m_EnableScreenSpaceWaterReflection`; picks the distant-reflection
    /// binding over the full one. `None` when the manager singleton is unavailable.
    pub screen_space_reflection: Option<bool>,
    /// Whether the per-eye reflection re-mirror was configured for this frame (issue #47).
    pub per_eye_reflection: bool,
    /// The reflection camera's world position at this eye's water draw, to verify the per-eye
    /// re-mirror actually landed (the two eyes should differ by the mirrored eye delta). `None`
    /// when the manager or camera is unavailable.
    pub reflection_camera_position: Option<[f32; 3]>,
    /// The reflection camera's world forward row at this eye's water draw, for the rotational part
    /// of the same verification.
    pub reflection_camera_forward: Option<[f32; 3]>,
    /// How many passes in the reflection chain (categories 9..=16) were enabled when this
    /// dispatch's pre-pass loop ran; zero on an eye that skipped them means the re-render never
    /// happened. `None` until the pre-draw hook records it.
    pub reflection_passes_enabled: Option<usize>,
    pub depth_texture: String,
    pub dynamic_reflection_color_texture: String,
    pub dynamic_reflection_alpha_texture: String,
    pub back_buffer_texture: String,
    /// The block type's shared textures and tessellation selector; `None` when the type singleton
    /// is unavailable.
    pub type_textures: Option<WaterTypeTextures>,
}

/// The [`NvWaterHighEndRenderBlockType`] singleton's shared bindings, as pointer identities.
#[derive(Clone, Serialize)]
pub struct WaterTypeTextures {
    pub tessellation_options: i32,
    pub water_mod: String,
    pub foam: String,
    pub water_bump: String,
    pub distant_reflection: String,
    pub full_reflection: String,
}

/// The latest [`WaterDrawSnapshot`] per eye (`draw_index`-indexed), for the screenshot sidecar.
pub fn last_water_draws() -> [Option<WaterDrawSnapshot>; 2] {
    LAST_WATER_DRAWS.lock().clone()
}

/// Re-mirror the water reflection camera for the dispatch's eye (issue #47). The water shader
/// samples its reflection maps at the pixel's own screen position, so the maps are only valid for
/// the exact camera they were rendered from; the engine mirrors one camera per frame (the centre
/// pose, in `CWaterPatchManager::UpdateThread`), which matches neither eye. This composes the
/// eye's rigid pose delta -- conjugated by the water-plane mirror, so it moves the mirrored camera
/// the way the mirrored eye moves -- onto the engine's own mirror, and the reflection pre-passes
/// re-render from it each dispatch (their sharing is carved out in
/// `render_pass::disable_shared_prepasses` while this is on).
///
/// Called from the `PreDraw` dispatch prologue on the draw thread, after the dispatch's eye camera
/// is established and before the reflection passes run. The engine's own per-frame mirror is
/// snapshotted at the frame's first dispatch so the second eye composes onto it rather than onto
/// the first eye's already-offset camera. A no-op under the single-pass collapse: its one dispatch
/// keeps the engine's centre mirror, whose error is at least symmetric between the eye-halves.
pub(crate) fn apply_per_eye_reflection_camera() {
    if !Config::lock_query(|c| c.stereo.per_eye_water_reflection)
        || !crate::stereo::active()
        || crate::stereo::single_pass::collapse_active()
    {
        return;
    }
    let eye = crate::stereo::draw_index();
    let Some(params) = crate::vr::render_params(eye) else {
        return;
    };
    let Some(center) = crate::stereo::STEREO_STATE.lock().center_transform else {
        return;
    };
    // SAFETY: the water-patch-manager singleton and its reflection camera are engine-owned and
    // null-checked; the camera's matrices are written on the draw thread at the dispatch prologue,
    // before the reflection passes that read them run on this same thread.
    unsafe {
        let Some(manager) = WaterPatchManager::get() else {
            return;
        };
        let Some(camera) = manager.m_ReflectionCamera.as_mut() else {
            return;
        };
        let center_world = glam::Mat4::from(center);
        let mut eye_world = center_world;
        eye_world.w_axis += params.world_offset.extend(0.0);
        let eye_world = eye_world * glam::Mat4::from_quat(params.orientation_delta);
        let mirrored = {
            let mut lock = REFLECTION_MIRROR_STATE.lock();
            let current = glam::Mat4::from(camera.m_TransformF);
            let state = lock.get_or_insert_with(|| MirrorState {
                engine_base: current,
                last_written: None,
            });
            // Adopt the camera as the frame's engine mirror only when the engine actually rewrote
            // it since our last write: `UpdateThread` skips its mirror rebuild in some camera
            // configurations, and adopting our own previous write as the base would compound the
            // eye delta frame over frame.
            if crate::stereo::dispatch_ordinal() == 0 && state.last_written != Some(current) {
                state.engine_base = current;
            }
            let mirrored =
                mirrored_eye_reflection_transform(state.engine_base, center_world, eye_world);
            state.last_written = Some(mirrored);
            mirrored
        };
        camera.m_TransformF = Matrix4::from(mirrored);
        camera.m_View = Matrix4::from(mirrored.inverse());
        // The engine rebuilt the reflection camera's projections on the game thread; only the pose
        // changes here, so rebuild the view-projections from the existing projections (`Matrix4`'s
        // `*` is the engine's row-major `Multiply4x4` convention).
        camera.m_ViewProjection = camera.m_View * camera.m_Projection;
        camera.m_ViewProjectionF = camera.m_View * camera.m_ProjectionF;
    }
}

/// Apply or lift the screen-space water-reflection override
/// ([`crate::stereo::config::StereoConfig::disable_screen_space_water_reflection`]): while
/// overridden, `WaterPatchManager::m_EnableScreenSpaceWaterReflection` is held false, and the
/// engine's own value is saved on entry and restored on exit. Called once per frame from
/// `game_update_render` on the game thread (before the engine's update reads the flag), mirroring
/// `apply_sun_shadow_override`'s sentinel discipline.
pub(crate) fn apply_ssr_override(disable: bool) {
    // SAFETY: the water-patch-manager singleton is live once the engine is initialised, and it is
    // null-checked; the flag is a plain settings toggle read by the game-thread water update and
    // the water draws.
    unsafe {
        let Some(manager) = WaterPatchManager::get() else {
            return;
        };
        let saved = SAVED_SSWR_ENABLED.load(Ordering::Relaxed);
        if disable && saved == u8::MAX {
            SAVED_SSWR_ENABLED.store(
                u8::from(manager.m_EnableScreenSpaceWaterReflection),
                Ordering::Relaxed,
            );
            manager.m_EnableScreenSpaceWaterReflection = false;
        } else if !disable && saved != u8::MAX {
            manager.m_EnableScreenSpaceWaterReflection = saved != 0;
            SAVED_SSWR_ENABLED.store(u8::MAX, Ordering::Relaxed);
        }
    }
}

/// The high-end water family (`waterhighend`, `waterbelow`, `watershader_lod0/1/2`) reads its
/// screen-UV matrix from vertex `cb1` registers 1..4, staged from the render context's
/// world→clip view-projection.
#[detour(address = jc3gi::graphics_engine::render_block::WaterHighEndRenderBlock::Draw_ADDRESS)]
fn water_high_end_draw(
    this: *const WaterHighEndRenderBlock,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let detour = WATER_HIGH_END_DRAW.get().unwrap();
    // SAFETY: `rc` is the live render context the engine passed into `Draw`, read only for its
    // matrices; the closure re-invokes the original `Draw` trampoline.
    let handled = unsafe {
        per_eye(
            rc,
            HIGH_END_REGISTER,
            |rc| rc.m_ViewProjectionF.data,
            || {
                detour.call(this, rc, info);
            },
        )
    };
    if !handled {
        detour.call(this, rc, info);
    }
}

/// The water-box family (`waterbox`, `waterboxbelow`, `waterboxclear`) reads the same kind of matrix
/// from vertex `cb1` registers 4..7, staged from the translation-free view-projection instead --
/// its geometry is camera-relative.
#[detour(address = jc3gi::graphics_engine::render_block::WaterBoxRenderBlock::Draw_ADDRESS)]
fn water_box_draw(this: *const WaterBoxRenderBlock, rc: *mut RenderContext, info: *const RBIInfo) {
    let detour = WATER_BOX_DRAW.get().unwrap();
    // SAFETY: as above.
    let handled = unsafe {
        per_eye(
            rc,
            BOX_REGISTER,
            |rc| rc.m_OffsetViewProjection.data,
            || detour.call(this, rc, info),
        )
    };
    if !handled {
        detour.call(this, rc, info);
    }
}

/// The WaveWorks family, re-issued once per eye with that eye's camera substituted into the render
/// context the block bakes its matrices from. See [`nv_water_per_eye`].
#[detour(address = jc3gi::graphics_engine::render_block::NvWaterHighEndRenderBlock::Draw_ADDRESS)]
fn nv_water_high_end_draw(
    this: *const NvWaterHighEndRenderBlock,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    // SAFETY: `this` and `rc` are the live block and render context the engine passed into `Draw`.
    unsafe { snapshot_water_draw(this, rc) };
    let detour = NV_WATER_HIGH_END_DRAW.get().unwrap();
    // SAFETY: `this` and `rc` are the live block and render context the engine passed into `Draw`; the
    // closure re-invokes the original `Draw` trampoline.
    let handled = unsafe {
        nv_water_per_eye(this, rc, || {
            detour.call(this, rc, info);
        })
    };
    if !handled {
        detour.call(this, rc, info);
    }
}

/// The WaveWorks simulation step, run at most once per frame: suppressed on the second eye of an
/// [`nv_water_per_eye`] re-issue, and on every stereo dispatch after the frame's first water draw
/// ([`SIMULATION_STEPPED_THIS_FRAME`]).
///
/// The engine calls this once per frame from the main-body water draw, and the call is the only part
/// of that draw that is not idempotent: it advances the simulation clock, blocks on the readback
/// staging cursor, and archives another displacement snapshot into the ring the CPU-side wave-height
/// and buoyancy queries read. Repeating it within a frame halves that ring's time span, pays the
/// stall again -- and, across the two-pass stereo dispatches, hands the second eye a later ocean than
/// the first, decorrelating the normal-sensitive sun glint between the eyes (issue #47).
#[detour(address = jc3gi::graphics_engine::render_block::WaveWorksSimulationStep_ADDRESS)]
extern "system" fn wave_works_simulation_step(
    render_time: f64,
    gfx_context: *mut c_void,
    kick_id: *mut u64,
    simulation: *mut c_void,
    savestate: *mut c_void,
) {
    if SUPPRESS_SIMULATION_STEP.load(Ordering::Relaxed) {
        return;
    }
    // The swap must come last: it may only latch the step as taken when stereo is active and the
    // sharing flag is on, so a flat frame (or an A/B with the flag off) keeps stock behaviour.
    if crate::stereo::active()
        && Config::lock_query(|c| c.stereo.share_water_simulation)
        && SIMULATION_STEPPED_THIS_FRAME.swap(true, Ordering::Relaxed)
    {
        return;
    }
    WAVE_WORKS_SIMULATION_STEP.get().unwrap().call(
        render_time,
        gfx_context,
        kick_id,
        simulation,
        savestate,
    );
}

/// The vertex constant buffer the water block types stage their screen-UV matrix into.
const CONSTANT_BUFFER: i32 = 1;

/// The first register of the high-end family's screen-UV matrix (`TypeConstants.ReflectionViewProj`).
const HIGH_END_REGISTER: u32 = 1;

/// The first register of the water-box family's screen-UV matrix (`cbWaterConsts.WaterConsts[4..7]`).
const BOX_REGISTER: u32 = 4;

/// Re-issue `draw` once per eye with the eye-half-biased screen-UV matrix, or return `false` when the
/// flag is off, the render context is unreadable, or the collapse intercept declines.
///
/// `view_projection` picks the render-context matrix the block type bakes the matrix from; this
/// recomputes the type's staged rows rather than intercepting them, because the type stages them once
/// per pass, well before the `Draw` being re-issued.
///
/// # Safety
///
/// `rc` must be the live [`RenderContext`] the detoured `Draw` received, and `draw` must invoke the
/// block's original `Draw` trampoline.
unsafe fn per_eye(
    rc: *mut RenderContext,
    reg_offset: u32,
    view_projection: impl Fn(&RenderContext) -> [f32; 16],
    draw: impl FnMut(),
) -> bool {
    if !Config::lock_query(|c| c.stereo.single_pass.water_uv_per_eye) {
        return false;
    }
    // SAFETY: `rc` is live per the caller contract.
    let Some(view_projection) = (unsafe { rc.as_ref() }).map(view_projection) else {
        return false;
    };
    // SAFETY: as above; `draw` is the block's original `Draw`.
    unsafe {
        crate::stereo::single_pass::screen_uv_cb_per_eye(
            rc,
            CONSTANT_BUFFER,
            reg_offset,
            screen_uv_matrix(view_projection),
            draw,
        )
    }
}

/// The rows the block type stages: `view_projection · TEX_BIAS`, in the engine's row-major storage.
///
/// Loading a row-major matrix into glam's column-major reading yields its transpose, and the
/// row-vector product `A · B` transposes to `Bᵀ · Aᵀ` -- so the factors swap and the result reads back
/// row-major unchanged.
fn screen_uv_matrix(view_projection: [f32; 16]) -> [f32; 16] {
    (glam::Mat4::from_cols_array(&TEX_BIAS) * glam::Mat4::from_cols_array(&view_projection))
        .to_cols_array()
}

/// The NDC→texture bias the water block types post-multiply their view-projection by: the standard
/// `xy · 0.5 + w · 0.5` with the depth row left alone, folded in on the CPU so the shader can divide
/// the interpolated result by `w` and sample directly. Row-major, row-vector convention (the
/// translation is the last row).
#[rustfmt::skip]
const TEX_BIAS: [f32; 16] = [
    0.5, 0.0, 0.0, 0.0,
    0.0, 0.5, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    0.5, 0.5, 0.0, 1.0,
];

/// Re-issue the WaveWorks block's `Draw` once per eye with that eye's camera, or return `false` when
/// the flag is off, the pass is one of the block's auxiliary passes, the per-eye camera is
/// unavailable, or the collapse intercept declines.
///
/// Every `NvWater*` permutation writes clip position from a `g_ModelViewProjectionMatrix` at the
/// block type's *own* vertex/domain `cb1` registers 0..3, and its hull and domain shaders carry the
/// same epilogue -- none of them touch the global `cb0` the collapse's shader rewrite works on, so no
/// amount of `cb13`/viewport routing reaches them. The matrix is built in
/// [`NvWaterHighEndRenderBlock::Setup`] from exactly two render-context fields, `m_View` and
/// `m_ProjectionF`, and those same two matrices are also handed to WaveWorks itself as the view and
/// projection its quadtree culls and picks patch LODs against.
///
/// So rather than reproject a constant in flight, this substitutes the *inputs*: it writes the eye's
/// view and projection into the render context, calls the block's own `Setup` to rebuild and re-upload
/// everything downstream of them, and re-issues `Draw` -- once per eye, each into that eye's half of
/// the double-wide target. The render context is restored afterwards.
///
/// What that does *not* restore is the block's own cached matrices and the shared constant buffer,
/// which are left holding the right eye's. Nothing reads them before the next `Setup`: the draw-list
/// walk calls `Setup` before the first `Draw` of a block-type run and again whenever the sort id
/// changes, and every `Draw` in between comes back through here and restages per eye anyway. This is
/// verified by the pyxis definitions for `NvWaterHighEndRenderBlock::Setup` ("Neither buffer is
/// written anywhere else, and `Draw` restages nothing") and `NvWaterHighEndRenderBlock::Draw` (which
/// hands the same two block-held matrices straight to WaveWorks without restaging).
///
/// # Safety
///
/// `this` and `rc` must be the live pointers the detoured `Draw` received, and `draw` must invoke the
/// block's original `Draw` trampoline.
unsafe fn nv_water_per_eye(
    this: *const NvWaterHighEndRenderBlock,
    rc: *mut RenderContext,
    mut draw: impl FnMut(),
) -> bool {
    if !Config::lock_query(|c| c.stereo.single_pass.nvwater_per_eye) {
        return false;
    }
    // SAFETY: `rc` is the live render context per the caller contract.
    let (pass, center_view) = unsafe { ((*rc).m_ActiveRenderPass, (*rc).m_View) };
    if NV_WATER_AUXILIARY_PASSES.contains(&pass) {
        return false;
    }
    let Some(eyes) = eye_cameras(center_view) else {
        return false;
    };
    // SAFETY: as above.
    let saved_projection = unsafe { (*rc).m_ProjectionF };

    // The guard restores the render context's view/projection and clears the simulation-suppression
    // flag on drop, so engine state is consistent even if the per-eye closure unwinds.
    let _restore = RenderContextRestore {
        rc,
        center_view,
        saved_projection,
    };

    crate::stereo::single_pass::draw_per_eye_half_ignoring_bound_vs(|eye| {
        // SAFETY: `rc` is live, and `this` is the live block whose `Setup` reads it. The two trailing
        // arguments are the draw-list sort ids, which this block's `Setup` override does not read.
        unsafe {
            (*rc).m_View = eyes[eye].view;
            (*rc).m_ProjectionF = eyes[eye].projection;
            SUPPRESS_SIMULATION_STEP.store(eye != 0, Ordering::Relaxed);
            (*this).Setup(rc, 0, 0);
        }
        draw();
    })
}

/// The passes whose `CNvWaterHighEndRenderBlock::Draw` body is not the water surface: the compute
/// foam sub-pass, the wake prepass, and the painted-foam prepass. They render into the block's own
/// simulation and foam targets from their own viewports rather than into the scene, so the eye split
/// does not apply to them and `Setup` stages no view matrix for them either.
const NV_WATER_AUXILIARY_PASSES: [i32; 3] = [
    RenderPassId::PRE_RP_WATER_CS_PRE as i32,
    RenderPassId::PRE_RP_WATER_WAKES_PRE as i32,
    RenderPassId::PRE_RP_WATER_FOAM_PRE as i32,
];

/// Raised for the duration of the second eye's re-issue; see [`wave_works_simulation_step`].
static SUPPRESS_SIMULATION_STEP: AtomicBool = AtomicBool::new(false);

/// A frame ordinal advanced by [`begin_dispatch`] at each frame's first dispatch, stamped into the
/// [`WaterDrawSnapshot`]s so the sidecar can tell same-frame snapshots from stale ones.
static WATER_FRAME: AtomicU64 = AtomicU64::new(0);

/// The engine's own `m_EnableScreenSpaceWaterReflection` value while [`apply_ssr_override`] holds
/// it overridden, or `u8::MAX` when no override is active.
static SAVED_SSWR_ENABLED: AtomicU8 = AtomicU8::new(u8::MAX);

/// The engine's own per-frame reflection-camera mirror, and the transform this module last wrote
/// over it, so [`apply_per_eye_reflection_camera`] can tell an engine rebuild from its own
/// leftover write. Draw-thread only.
struct MirrorState {
    engine_base: glam::Mat4,
    last_written: Option<glam::Mat4>,
}

/// See [`MirrorState`].
static REFLECTION_MIRROR_STATE: Mutex<Option<MirrorState>> = Mutex::new(None);

/// How many reflection-chain passes (categories 9..=16) were enabled for the current dispatch's
/// pre-pass loop, recorded by the `PreDraw` hook via [`record_reflection_passes_enabled`] and read
/// into the [`WaterDrawSnapshot`].
static REFLECTION_PASSES_ENABLED: AtomicUsize = AtomicUsize::new(usize::MAX);

/// Record the dispatch's enabled reflection-pass count for the diagnostics snapshot.
pub(crate) fn record_reflection_passes_enabled(count: usize) {
    REFLECTION_PASSES_ENABLED.store(count, Ordering::Relaxed);
}

/// The per-eye reflection-camera transform: the eye's rigid motion relative to the centre
/// (`eye = delta ∘ center`, in world space), conjugated by the water-plane mirror, applied to the
/// engine's own mirror `base`. The conjugation moves the mirrored camera exactly as the mirrored
/// eye moves relative to the mirrored centre, so no assumption about the engine's mirrored-camera
/// orientation convention is needed. The plane height is inferred from the engine's own mirror
/// (`base.y = 2 * plane - center.y`) rather than read from engine state, so the
/// screen-space-reflection path's own plane convention is honoured automatically.
fn mirrored_eye_reflection_transform(
    base: glam::Mat4,
    center_world: glam::Mat4,
    eye_world: glam::Mat4,
) -> glam::Mat4 {
    let delta = eye_world * center_world.inverse();
    let plane_y = (base.w_axis.y + center_world.w_axis.y) * 0.5;
    let mirror = glam::Mat4::from_translation(glam::Vec3::Y * plane_y)
        * glam::Mat4::from_scale(glam::Vec3::new(1.0, -1.0, 1.0))
        * glam::Mat4::from_translation(glam::Vec3::Y * -plane_y);
    mirror * delta * mirror * base
}

/// The latest main-body water draw snapshot per eye. On a share frame the far dispatch's snapshot
/// (also `draw_index` 0) is overwritten by eye 0's near dispatch, which is the one the sidecar
/// wants; the `dispatch_ordinal` field records which one survived.
static LAST_WATER_DRAWS: Mutex<[Option<WaterDrawSnapshot>; 2]> = Mutex::new([None, None]);

/// Record the inputs of a main-body water draw into [`LAST_WATER_DRAWS`] under the current
/// `draw_index`. A no-op for the block's auxiliary passes, which stage none of these bindings.
///
/// # Safety
///
/// `this` and `rc` must be the live pointers the detoured `Draw` received.
unsafe fn snapshot_water_draw(this: *const NvWaterHighEndRenderBlock, rc: *const RenderContext) {
    // SAFETY: live per the caller contract; the singletons are null-checked by their accessors.
    unsafe {
        let (Some(block), Some(rc)) = (this.as_ref(), rc.as_ref()) else {
            return;
        };
        if NV_WATER_AUXILIARY_PASSES.contains(&rc.m_ActiveRenderPass) {
            return;
        }
        let reflection_camera = WaterPatchManager::get()
            .and_then(|m| m.m_ReflectionCamera.as_ref())
            .map(|cam| cam.m_TransformF.data);
        let snapshot = WaterDrawSnapshot {
            frame: WATER_FRAME.load(Ordering::Relaxed),
            dispatch_ordinal: crate::stereo::dispatch_ordinal(),
            render_pass: rc.m_ActiveRenderPass,
            under_water: block.m_UnderWater,
            altitude_simulation_pause: block.m_AltitudeSimulationPause,
            render_time: block.m_RenderTime,
            screen_space_reflection: WaterPatchManager::get()
                .map(|m| m.m_EnableScreenSpaceWaterReflection),
            per_eye_reflection: Config::lock_query(|c| c.stereo.per_eye_water_reflection),
            reflection_camera_position: reflection_camera.map(|t| [t[12], t[13], t[14]]),
            reflection_camera_forward: reflection_camera.map(|t| [t[8], t[9], t[10]]),
            reflection_passes_enabled: match REFLECTION_PASSES_ENABLED.load(Ordering::Relaxed) {
                usize::MAX => None,
                n => Some(n),
            },
            depth_texture: ptr_hex(rc.m_DepthBufferTexture),
            dynamic_reflection_color_texture: ptr_hex(rc.m_DynamicReflectionColorTexture),
            dynamic_reflection_alpha_texture: ptr_hex(rc.m_DynamicReflectionAlphaTexture),
            back_buffer_texture: ptr_hex(rc.m_BackBufferTexture),
            type_textures: NvWaterHighEndRenderBlockType::get().map(|t| WaterTypeTextures {
                tessellation_options: t.m_TessellationOptions,
                water_mod: ptr_hex(t.m_WaterModTexture),
                foam: ptr_hex(t.m_FoamTexture),
                water_bump: ptr_hex(t.m_WaterBumpTexture),
                distant_reflection: ptr_hex(t.m_DistantReflectionTexture),
                full_reflection: ptr_hex(t.m_FullReflectionTexture),
            }),
        };
        if let Some(slot) = LAST_WATER_DRAWS.lock().get_mut(crate::stereo::draw_index()) {
            *slot = Some(snapshot);
        }
    }
}

/// A raw pointer as a hex-string identity for the sidecar (null for a null pointer).
fn ptr_hex<T>(ptr: *mut T) -> String {
    format!("{:#x}", ptr as usize)
}

/// Whether the WaveWorks simulation has already stepped this frame: cleared at the frame's first
/// dispatch by [`begin_dispatch`], latched by [`wave_works_simulation_step`] when it runs the step.
/// Keyed to the frame's first *water draw* rather than to a fixed dispatch ordinal because which
/// dispatch that is depends on the frame shape -- a share frame's far dispatch may or may not carry
/// the ocean -- and suppressing on ordinal alone could silence the step for the whole frame,
/// freezing the simulation. Written only on the draw thread.
static SIMULATION_STEPPED_THIS_FRAME: AtomicBool = AtomicBool::new(false);

/// Restores the render context's view/projection and clears the simulation-suppression flag on drop,
/// so engine state is consistent even if the per-eye closure in [`nv_water_per_eye`] unwinds.
struct RenderContextRestore {
    rc: *mut RenderContext,
    center_view: Matrix4,
    saved_projection: Matrix4,
}

impl Drop for RenderContextRestore {
    fn drop(&mut self) {
        SUPPRESS_SIMULATION_STEP.store(false, Ordering::Relaxed);
        // SAFETY: `rc` is the live render context that was valid when the guard was constructed, and
        // the render thread still owns it during drop on the same unwind path.
        unsafe {
            (*self.rc).m_View = self.center_view;
            (*self.rc).m_ProjectionF = self.saved_projection;
        }
    }
}

/// One eye's substitute for the render context's camera matrices.
struct EyeCamera {
    view: Matrix4,
    projection: Matrix4,
}

/// The two eyes' view and projection matrices, derived from the collapse's centre view the same way
/// the `SetupRenderCamera` hook derives the double-draw path's per-eye camera: offset the centre
/// camera's world transform by the eye's world offset, apply its head-local orientation delta on the
/// right (about the now-offset eye position), and invert. `None` when no VR frame is in flight.
fn eye_cameras(center_view: Matrix4) -> Option<[EyeCamera; 2]> {
    let center_world = glam::Mat4::from(center_view).inverse();
    Some([eye_camera(center_world, 0)?, eye_camera(center_world, 1)?])
}

fn eye_camera(center_world: glam::Mat4, eye: usize) -> Option<EyeCamera> {
    let params = crate::vr::render_params(eye)?;
    let mut world = center_world;
    world.w_axis += params.world_offset.extend(0.0);
    let world = world * glam::Mat4::from_quat(params.orientation_delta);
    Some(EyeCamera {
        view: Matrix4::from(world.inverse()),
        // The reverse-Z form, which is what the render camera's `m_ProjectionF` holds by the time a
        // render context is filled from it, under either projection convention.
        projection: params.projection_reverse_z,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// When the base is exactly the plane mirror of the centre (no engine orientation fixup), the
    /// conjugated result must be exactly the plane mirror of the eye: `M·D·M·(M·C) = M·E`.
    #[test]
    fn eye_reflection_transform_mirrors_the_eye() {
        let plane_y = 2.0;
        let mirror = glam::Mat4::from_translation(glam::Vec3::Y * plane_y)
            * glam::Mat4::from_scale(glam::Vec3::new(1.0, -1.0, 1.0))
            * glam::Mat4::from_translation(glam::Vec3::Y * -plane_y);
        let center = glam::Mat4::from_rotation_translation(
            glam::Quat::from_rotation_y(0.4),
            glam::Vec3::new(3.0, 10.0, -7.0),
        );
        let mut eye = center;
        eye.w_axis += glam::Vec4::new(0.034, 0.004, 0.01, 0.0);
        let eye = eye * glam::Mat4::from_quat(glam::Quat::from_rotation_x(0.05));

        let base = mirror * center;
        // The plane inference recovers y = 2 from `base` and `center` alone.
        assert!(((base.w_axis.y + center.w_axis.y) * 0.5 - plane_y).abs() < 1e-6);

        let result = mirrored_eye_reflection_transform(base, center, eye);
        let expected = mirror * eye;
        assert!(
            result.abs_diff_eq(expected, 1e-5),
            "{result:?} vs {expected:?}"
        );
    }

    /// The result's position is the eye position reflected across the water plane, regardless of
    /// the base's own orientation convention (here an arbitrary proper rotation).
    #[test]
    fn eye_reflection_transform_reflects_the_position() {
        let center = glam::Mat4::from_translation(glam::Vec3::new(0.0, 10.0, 0.0));
        let mut eye = center;
        eye.w_axis += glam::Vec4::new(0.034, 0.004, 0.01, 0.0);
        // The engine's mirror of the centre about y = 2, with a proper-rotation orientation as
        // `CreateOrientation` produces.
        let base = glam::Mat4::from_rotation_translation(
            glam::Quat::from_rotation_x(std::f32::consts::PI),
            glam::Vec3::new(0.0, -6.0, 0.0),
        );
        let result = mirrored_eye_reflection_transform(base, center, eye);
        let expected = glam::Vec3::new(0.034, 2.0 * 2.0 - 10.004, 0.01);
        assert!(
            result.w_axis.truncate().abs_diff_eq(expected, 1e-5),
            "{:?} vs {expected:?}",
            result.w_axis
        );
    }

    /// The composed transform maps a clip position into the eye's half of the double-wide target: the
    /// screen-UV matrix's own NDC→UV bias, then the eye-half bias the per-eye re-issue applies.
    #[test]
    fn eye_half_bias_maps_ndc_into_the_eyes_half() {
        // A clip position with a non-unit `w`, to catch a bias applied to the post-divide `u` instead
        // of to the projective components.
        let clip = glam::Vec4::new(0.5, -0.25, 0.75, 2.0);
        let rows = screen_uv_matrix(glam::Mat4::IDENTITY.to_cols_array());
        let row = |k: usize| glam::Vec4::from_slice(&rows[k * 4..k * 4 + 4]);

        for eye in 0..2 {
            // The row-vector product with the per-eye bias `screen_uv_cb_per_eye` composes.
            let biased = |k: usize| {
                let r = row(k);
                glam::Vec4::new(r.x * 0.5 + r.w * 0.5 * eye as f32, r.y, r.z, r.w)
            };
            let projective: glam::Vec4 = (0..4).map(|k| clip[k] * biased(k)).sum();
            // The vertex shader packs `(x, y, w)` into `TEXCOORD1` and the pixel shader divides the
            // first two by the third.
            let uv = glam::Vec2::new(projective.x, projective.y) / projective.w;

            let ndc = glam::Vec2::new(clip.x, clip.y) / clip.w;
            let expected_u = ((ndc.x * 0.5 + 0.5) + eye as f32) * 0.5;
            assert!((uv.x - expected_u).abs() < 1e-6, "eye {eye}: {uv:?}");
            assert!(
                (uv.y - (ndc.y * 0.5 + 0.5)).abs() < 1e-6,
                "eye {eye}: {uv:?}"
            );
        }
    }
}
