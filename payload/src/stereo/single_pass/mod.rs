//! Single-pass stereo (experimental): render the G-buffer geometry once, emitting both eyes via
//! instancing + `SV_ViewportArrayIndex` routing into a double-wide render target, instead of the
//! double-draw (two full `game.Draw` walks, one per eye). See `docs/mod/single-pass-stereo.md` for
//! the design.
//!
//! This module owns the mod-side state that the double-draw path does not need:
//! - the DXVK viewport-routing **capability probe** ([`probe`] / [`capability`]);
//! - the vertex-shader rewrite **census** ([`record_patch_outcome`] and the `*_count` getters), which
//!   the `CreateVertexProgram` hook feeds so the debug UI can report how the rewriter fared against
//!   the game's real shader set;
//! - which **transform** each vertex shader gets ([`decide_vs_transform`]), decided from its engine
//!   name at `CreateVertexProgram` time and remembered against its bytecode ([`remember_vs_transform`])
//!   so the D3D-level re-create path, which sees no name, applies the same decision instead of
//!   guessing a different one.
//!
//! The rest of the pipeline (cb13 dual-eye upload, the double-wide render-setup re-init, the
//! draw-doubling) runs under [`crate::config::StereoConfig::single_pass`] and the per-step flags
//! beside it. [`crate::config::StereoConfig::single_pass_patch_dryrun`] runs the census alone, with
//! no rendering change.

use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicU32, AtomicUsize, Ordering},
};

use dxbc_stereo::DxbcError;
use jc3gi::{
    graphics_engine::{
        draw::SetVertexProgramConstants,
        graphics_engine::{GraphicsEngine, HContext_t, RenderContext},
        render_block::RenderBlockTerrainDetail,
        render_engine::{RenderEngine, RenderPassId},
    },
    types::math::{Matrix4, Vector4},
};
use parking_lot::Mutex;
use re_utilities::ThreadSuspender;
use retour::{Function, GenericDetour};
use windows::{
    Win32::{
        Foundation::RECT,
        Graphics::Direct3D11::{
            D3D11_BIND_CONSTANT_BUFFER, D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_WRITE,
            D3D11_FEATURE_D3D11_OPTIONS3, D3D11_FEATURE_DATA_D3D11_OPTIONS3,
            D3D11_MAP_WRITE_DISCARD, D3D11_MAPPED_SUBRESOURCE, D3D11_SUBRESOURCE_DATA,
            D3D11_USAGE_DYNAMIC, D3D11_VIEWPORT, ID3D11Buffer, ID3D11Device, ID3D11DeviceContext,
        },
        System::Threading::{EnterCriticalSection, LeaveCriticalSection},
    },
    core::{IUnknown, Interface},
};

use crate::config::Config;

/// The result of the DXVK viewport-routing capability probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// Not yet probed (no device seen, or the probe has not run this session).
    Unprobed,
    /// `VPAndRTArrayIndexFromAnyShaderFeedingRasterizer` is supported: a vertex shader may write
    /// `SV_ViewportArrayIndex` directly, so single-pass routing is possible.
    Supported,
    /// The capability is absent; single-pass must fall back to double-draw.
    Unsupported,
}

/// Probe the D3D11 device for `VPAndRTArrayIndexFromAnyShaderFeedingRasterizer` (the D3D11.3 feature
/// that lets a vertex shader write `SV_ViewportArrayIndex`), caching the result. Idempotent and
/// cheap; safe to call every frame. `CheckFeatureSupport` on the device is free-threaded, so no
/// context lock is needed.
mod cb13;
mod config_snapshot;
mod draw_detours;
mod frame_diagnostics;
mod instanced_exposure;
mod per_eye_reissue;
mod shader_detours;
mod shader_policy;
mod viewport;

pub use cb13::*;
pub use config_snapshot::*;
use draw_detours::*;
pub use frame_diagnostics::*;
pub use instanced_exposure::*;
pub use per_eye_reissue::*;
pub use shader_detours::*;
pub use shader_policy::*;
pub use viewport::*;

pub(crate) use crate::stereo::engine_context::EngineContext;

pub fn probe(device: &ID3D11Device) -> Capability {
    let mut options = D3D11_FEATURE_DATA_D3D11_OPTIONS3::default();
    let ok = unsafe {
        device.CheckFeatureSupport(
            D3D11_FEATURE_D3D11_OPTIONS3,
            std::ptr::from_mut(&mut options).cast(),
            std::mem::size_of::<D3D11_FEATURE_DATA_D3D11_OPTIONS3>() as u32,
        )
    };
    let capability = if ok.is_ok()
        && options
            .VPAndRTArrayIndexFromAnyShaderFeedingRasterizer
            .as_bool()
    {
        Capability::Supported
    } else {
        Capability::Unsupported
    };
    CAPABILITY.store(capability as u8, Ordering::Relaxed);
    capability
}

/// Probe the capability using the live engine device, if one is available and the probe has not run
/// yet. Returns the (now cached) result. Called from the debug UI and the frame driver so the probe
/// happens as soon as a device exists.
pub fn probe_if_needed() -> Capability {
    let cached = capability();
    if cached != Capability::Unprobed {
        return cached;
    }
    // SAFETY: `GraphicsEngine::get` returns the live singleton or `None`; the device pointer is stable
    // once the engine has initialised.
    let Some(device) = (unsafe { GraphicsEngine::get() }) else {
        return Capability::Unprobed;
    };
    let Some(device) = (unsafe { device.m_Device.as_ref() }) else {
        return Capability::Unprobed;
    };
    probe(&device.m_Device)
}

/// The cached capability-probe result.
pub fn capability() -> Capability {
    match CAPABILITY.load(Ordering::Relaxed) {
        x if x == Capability::Supported as u8 => Capability::Supported,
        x if x == Capability::Unsupported as u8 => Capability::Unsupported,
        _ => Capability::Unprobed,
    }
}

/// Whether single-pass rendering should actually run this frame: the master switch is on, the
/// census-only dry-run is off, and the device supports viewport routing. The VS-substitution and
/// cb13 paths gate on this; when it is false the double-draw path is left untouched.
pub fn active() -> bool {
    // Go inert the instant eject begins: the render thread keeps running through the whole teardown,
    // so an ungated single-pass path would race the hook uninstall and the D3D-resource release (the
    // same crash-on-uninject class already fixed for `vr::update` -- see `crate::is_shutting_down`).
    if crate::is_shutting_down() {
        return false;
    }
    let flags = config_flags();
    flags.has(Flag::SinglePass) && !flags.has(Flag::DryRun) && capability() == Capability::Supported
}

/// Whether the eyes are made to diverge (in addition to [`active`]): distinct per-eye `cb13`,
/// left/right-half viewport routing, and instance doubling of the G-buffer geometry. With it off the
/// patched shaders still run, but both `cb13` eye slots hold the same view, so the two eyes render
/// identically -- the shape the substitution was brought up in, and still the fallback whenever
/// [`compute_dual_eye_rows`] cannot produce per-eye data.
pub fn dual_eye_active() -> bool {
    active() && config_flags().has(Flag::DualEye)
}

/// Whether the per-eye double-draw has been collapsed to a single G-buffer walk: one `game.Draw`
/// produces both eyes (via [`dual_eye_active`]'s `cb13` + viewport routing + instance doubling), the
/// render camera stays centered (no per-eye offset -- both eyes come from `cb13`), and the capture
/// splits the one back buffer into the two eye textures. Requires [`dual_eye_active`]; independent of
/// `single_pass_double_wide`, which only upgrades each eye-half from squished to full resolution.
pub fn collapse_active() -> bool {
    dual_eye_active() && config_flags().has(Flag::Collapse)
}

/// Whether the scene render targets are re-created at 2x per-eye width so each eye-half is full
/// resolution (instead of a squished half of a per-eye-sized target). Requires [`collapse_active`] --
/// it only makes sense for the single walk whose capture split reads one full-width half per eye.
/// Drives the engine render resolution ([`crate::vr::engine_render_resolution`]) and the per-eye
/// capture-texture width (`ui::render`); the XR swapchain stays per-eye width.
pub fn double_wide_active() -> bool {
    collapse_active() && config_flags().has(Flag::DoubleWide)
}

/// Whether the per-eye re-issue intercept for one of the baked-view-projection render blocks is
/// enabled. Read from the frame's config snapshot, so the block `Draw` detours -- which fire for
/// every draw of their type whether or not single-pass is on -- cost a relaxed load rather than a
/// mutex acquisition.
pub fn block_intercept_enabled(block: BlockIntercept) -> bool {
    config_flags().has(match block {
        BlockIntercept::Bark => Flag::Bark,
        BlockIntercept::Foliage => Flag::Foliage,
        BlockIntercept::Occluder => Flag::Occluder,
    })
}

/// Vertex-shader name prefixes whose draws a baked-view-projection block intercept owns end to end,
/// with the intercept's own flag. Names come from `CreateVertexProgramParams.m_Name`; matched by
/// prefix to cover each family's permutations, all of which are issued by the block's `Draw`/`DrawZ`.
///
/// The occluder is deliberately absent: its shader has no `cb0[4]` reference, so the remap never
/// claims it and there is nothing to decline.
const BAKED_CB_VS_NAME_PREFIXES: &[(&str, BlockIntercept)] = &[
    ("vegetationbark", BlockIntercept::Bark),
    ("vegetationfoliage", BlockIntercept::Foliage),
];

/// Whether a baked-view-projection block intercept owns this vertex shader, so the `cb0` remap must
/// leave it pristine.
///
/// Every vegetation vertex shader that reads `cb0` reads **only** `cb0[4]`, the camera world position
/// -- the foliage family as the world-space origin of its wind-noise lookup, the bark family as the
/// offset paired with a view-projection baked into `cb1`. Neither takes its clip position from `cb0`
/// (`dcl_constantbuffer CB0[5]` cannot even address the view-projection rows), so the remap gives them
/// no per-eye clip: both eyes keep the collapsed centre view, and near geometry rendered at zero
/// disparity reads as swimming against the parallaxed world around it. Being remapped also costs them
/// the fix: the intercept that *does* reproject their baked matrix stands down for a patched shader
/// ([`reproject_baked_cb_per_eye`]), because a patched shader is supposed to be producing both eyes
/// already.
///
/// So the two go together, exactly as they are gated: while the block's flag is on, its `Draw`/`DrawZ`
/// is re-issued per eye with the baked matrix reprojected, and its shaders are declined here. With the
/// flag off both halves revert and the family is remapped as before.
///
/// The bytecode is the final say, not the name: a permutation that really does read the global
/// view-projection is left to the remap, so a future bundle that moves one of these families onto
/// `cb0` does not silently lose its position path.
///
/// The decline is recorded as [`VsTransform::None`] against the blob, so the D3D-level re-create path
/// honours it too -- that path has no name to decline by, and left to itself it would remap the very
/// shaders this declined.
pub fn baked_cb_block_owns_vs(name: Option<&str>, code: &[u8]) -> bool {
    let Some(name) = name else {
        return false;
    };
    BAKED_CB_VS_NAME_PREFIXES
        .iter()
        .any(|(prefix, block)| name.starts_with(prefix) && block_intercept_enabled(*block))
        && dxbc_stereo::reads_global_view_projection(code).is_ok_and(|reads| !reads)
}

/// A render block with a baked-view-projection per-eye intercept.
#[derive(Clone, Copy)]
pub enum BlockIntercept {
    Bark,
    Foliage,
    Occluder,
}

/// The eye whose viewport + view-projection the collapse UI overlays (HUD panel, egui panel) should
/// currently draw with, so a head/world-locked quad lands at the correct 3D spot in each eye instead
/// of being drawn once, stretched, across the double-wide target. Set around each eye's overlay draw
/// by `render_engine_post_draw`; [`NO_UI_EYE`] means "not drawing a collapse overlay".
static COLLAPSE_UI_EYE: AtomicUsize = AtomicUsize::new(NO_UI_EYE);
const NO_UI_EYE: usize = usize::MAX;

/// Select the eye for the collapse UI overlay draws (`Some(0)`/`Some(1)`), or clear it (`None`).
pub fn set_collapse_ui_eye(eye: Option<usize>) {
    COLLAPSE_UI_EYE.store(eye.unwrap_or(NO_UI_EYE), Ordering::Relaxed);
}

/// The eye-half viewport and per-eye **full** view-projection for the current collapse UI overlay
/// draw, or `None` when not drawing one (or not collapsed). The HUD/egui-panel quad renderer uses
/// this to draw each overlay into one eye's half with that eye's own VP.
pub fn collapse_ui_eye_override() -> Option<(D3D11_VIEWPORT, Matrix4)> {
    let eye = COLLAPSE_UI_EYE.load(Ordering::Relaxed);
    if eye == NO_UI_EYE || !collapse_active() {
        return None;
    }
    let full = (*COLLAPSE_FULL_VIEWPORT.lock())?;
    let half = full.Width / 2.0;
    let viewport = D3D11_VIEWPORT {
        TopLeftX: full.TopLeftX + eye as f32 * half,
        Width: half,
        ..full
    };
    Some((viewport, full_eye_view_projection(eye)?))
}

/// Set the immediate-context viewport via the original (un-detoured) `RSSetViewports`, so a mod
/// overlay can bind one eye's half of the double-wide target without the collapse viewport detour
/// dup'ing it to full width. `context` is the raw `ID3D11DeviceContext` pointer.
pub fn set_ui_viewport_raw(context: *mut c_void, viewport: &D3D11_VIEWPORT) {
    if let Some(detour) = RS_SET_VIEWPORTS.get() {
        // One slot: slot 1 is left unbound, so the slots are no longer uniform.
        set_viewport_slots_uniform(false);
        // SAFETY: `context` is the live immediate context; the trampoline is the original function.
        unsafe { detour.call(context, 1, std::slice::from_ref(viewport).as_ptr()) };
    }
}

/// The per-eye **full** (translation-carrying) world→clip view-projection, matching the render
/// camera's `m_ViewProjectionF` for that eye -- for projecting the mod's world-space overlay quads
/// per eye in the collapse, where the render camera stays centered. `None` if the centre transform or
/// per-eye params are unavailable.
fn full_eye_view_projection(eye: usize) -> Option<Matrix4> {
    let center_transform = crate::stereo::STEREO_STATE.lock().center_transform?;
    let params = crate::vr::render_params(eye)?;
    let mut eye_world = glam::Mat4::from(center_transform);
    eye_world.w_axis += params.world_offset.extend(0.0);
    let eye_world = eye_world * glam::Mat4::from_quat(params.orientation_delta);
    let vp = glam::Mat4::from(params.projection_reverse_z) * eye_world.inverse();
    Some(Matrix4::from(vp))
}

/// Marks the render thread as inside the G-buffer geometry pass range
/// (`RP_Z_OCCLUDERS..RP_FIRST_SCENE`) until the returned guard drops. The dual-eye viewport split and
/// instance doubling apply only here -- so shadow/lighting/post passes, which reuse the same patched
/// shaders but are not double-wide, keep the identical-viewport behaviour.
///
/// A guard rather than a matching pair of calls: the range wraps a re-entrant engine call, and any
/// non-local exit from it (a panic unwinding through the detour, an early return added later) that
/// skipped the clear would leave the flag raised for the rest of the session -- after which *every*
/// shadow and reflection draw is instance-doubled and eye-split.
#[must_use = "the G-buffer range ends when the guard is dropped"]
pub fn enter_gbuffer_range() -> GBufferRange {
    IN_GBUFFER_RANGE.store(true, Ordering::Relaxed);
    GBufferRange(())
}

/// Holds [`in_gbuffer_range`] true; see [`enter_gbuffer_range`].
pub struct GBufferRange(());

impl Drop for GBufferRange {
    fn drop(&mut self) {
        // The guard is the only writer that should ever lower the flag. Finding it already down means
        // something else cleared it while the range was still running -- every draw between that point
        // and here was treated as out-of-range -- so count it rather than losing it in the clear.
        if !IN_GBUFFER_RANGE.swap(false, Ordering::Relaxed) {
            RANGE_TORN.fetch_add(1, Ordering::Relaxed);
        }
        // The per-eye matrices belong to the range that just ended; see [`clear_gbuffer_range`].
        *CURRENT_M_EYE.lock() = None;
        // The collapse's per-draw split ([`ensure_collapse_viewport`]) leaves the two slots holding
        // different eye halves, and that state outlives the range: everything drawn between here and
        // the next engine viewport bind would route its odd-parity instances into the other eye's half.
        // Put the slots back to a single region now, which also covers the draws nothing detours (the
        // GPU-indirect ones) -- the per-draw repair cannot see those.
        if collapse_active() {
            unify_viewport_slots();
        }
    }
}

/// Force the range closed, whether or not a guard is live, so a range left open by a torn-down or
/// interrupted dispatch cannot bleed into the next one. See [`begin_frame`] for the caller.
fn clear_gbuffer_range() {
    IN_GBUFFER_RANGE.store(false, Ordering::Relaxed);
    // The per-eye matrices belong to the range that just ended. Dropping them means a re-issue that
    // somehow runs outside a range -- or in a later frame where `compute_dual_eye_rows` declined to
    // publish -- reprojects with nothing rather than with a stale head pose.
    *CURRENT_M_EYE.lock() = None;
}

fn in_gbuffer_range() -> bool {
    IN_GBUFFER_RANGE.load(Ordering::Relaxed)
}

/// The render pass currently being walked, published by the `RenderPass::DoDraw` detour so the draw
/// detours -- which see only a D3D context -- can tell which pass a draw belongs to. [`NO_PASS`]
/// stands for "outside any pass"; no real id collides with it (`m_Index` is a byte and the engine's
/// highest pass is `0x96`).
static CURRENT_PASS: AtomicU8 = AtomicU8::new(NO_PASS);

const NO_PASS: u8 = 0xFF;

/// Publish the pass being drawn for the duration of one `DoDraw`. Returns the previous value so the
/// caller restores it rather than clearing, since a block-level re-issue can nest one pass inside
/// another.
///
/// Takes the engine's `m_Index` in its own `i16` form; an id outside the byte range is not a pass this
/// module can classify, so it reads as [`NO_PASS`] rather than truncating into a real pass's slot.
pub fn set_current_pass(pass: i16) -> u8 {
    let pass = u8::try_from(pass).unwrap_or(NO_PASS);
    CURRENT_PASS.swap(pass, Ordering::Relaxed)
}

/// Restore a pass id previously returned by [`set_current_pass`].
pub fn restore_current_pass(pass: u8) {
    CURRENT_PASS.store(pass, Ordering::Relaxed);
}

fn current_pass_id() -> u8 {
    CURRENT_PASS.load(Ordering::Relaxed)
}

/// Per-pass tally of non-indexed (`Draw`, slot 13) submissions seen inside a range while collapsed.
///
/// This exists to replace inference with measurement. Which passes actually reach slot 13 is the fact
/// that decides whether a family is being rasterised across the whole double-wide target instead of
/// one eye's half, and it is otherwise only obtainable from a frame capture. Indexed by pass id.
static SLOT13_BY_PASS: [AtomicU32; 256] = [const { AtomicU32::new(0) }; 256];

/// Drain the per-pass slot-13 census as `(pass_id, count)` for the passes that saw any, most frequent
/// first. Reported in the per-range diagnostic line.
fn drain_slot13_census() -> Vec<(u8, u32)> {
    let mut seen: Vec<(u8, u32)> = SLOT13_BY_PASS
        .iter()
        .enumerate()
        .filter_map(|(pass, count)| {
            let count = count.swap(0, Ordering::Relaxed);
            (count > 0).then_some((pass as u8, count))
        })
        .collect();
    seen.sort_unstable_by_key(|&(_, count)| std::cmp::Reverse(count));
    seen
}

/// Open a new real frame: advance the frame ordinal the diagnostics are keyed to, and fold the
/// previous frame's already-instanced exposure into the history.
///
/// The exposure counters are per *frame*, but the G-buffer range is entered once per
/// `DrawRenderPassRange` call -- three times per dispatch under the collapse (`DrawGBuffer`
/// `0x2F..0x55`, `Draw` `0x56..0x96`, `DrawPosteffects` `0x96..0x97`; see
/// `docs/mod/single-pass-stereo.md`). Folding them at a range boundary therefore cut the frame into
/// three unequal windows and reported each as if it were a frame; the fold belongs here, where a
/// frame actually begins.
pub fn begin_frame() {
    // The leaked-range clear deliberately does *not* happen here: it belongs to the thread that
    // raises and lowers the flag, and [`begin_dispatch`] does it there. With the frame tail deferred
    // this thread runs concurrently with the previous frame's still-walking dispatch, so clearing
    // from here tears a live range out from under it.
    //
    // The frame that just ended owns both the exposure fold and, if it was a diagnostic frame, the
    // trailing exposure line -- so it carries the same ordinal as its own range lines, which were
    // emitted before the ordinal advanced.
    let logged = diagnostic_frame();
    let exposure = accumulate_instanced_exposure();
    if logged {
        log_instanced_exposure(exposure);
    }
    FRAME_ORDINAL.fetch_add(1, Ordering::Relaxed);
}

/// Open a dispatch on the draw thread: pin this dispatch's config flags, and close a G-buffer range
/// left open by an interrupted dispatch, before this one's first range is entered.
///
/// This is the only place the range flag may be forced down from outside its guard. The flag is
/// written and read exclusively on the draw thread, and the dispatch prologue is the one point in
/// that thread's sequence where no range can be live -- so a clear here cannot interleave with a range
/// in progress, as the former frame-start clear on the game thread could once the frame tail was
/// deferred.
///
/// The config flags are pinned here for the same reason and at the same point. They gate state this
/// thread arms and restores in pairs -- the eye-half viewport, the armed constant reprojection, the
/// per-eye re-issue loops, the range guard's own viewport repair -- and a flag that moved between an
/// arm and its restore would leave that state raised for the rest of the frame. Sampled per dispatch
/// rather than per frame because a dispatch is the unit the draw thread actually walks: under the
/// collapse the game thread is already a frame ahead of it.
pub fn begin_dispatch() {
    pin_dispatch_config_flags();
    clear_gbuffer_range();
}

/// Whether this frame is one the per-frame single-pass diagnostics log on.
fn diagnostic_frame() -> bool {
    FRAME_ORDINAL
        .load(Ordering::Relaxed)
        .is_multiple_of(DIAGNOSTIC_FRAME_CADENCE)
}

/// How often the single-pass bring-up diagnostics log, in real frames. Every range of a logged frame
/// reports, so the frame's whole pass-range sequence appears together and can be read as one frame
/// rather than as unrelated samples.
const DIAGNOSTIC_FRAME_CADENCE: usize = 120;

/// A process-global slot for one installed detour: lock-free to read, and reclaimable at teardown.
///
/// A `OnceLock` cannot give its contents back, so a `OnceLock<GenericDetour<_>>` static can only leak
/// -- Rust statics are not dropped, and a detour's trampoline lives in a `VirtualAlloc` region that
/// outlives the unmapped payload, so every inject/eject cycle strands one page per detour. An
/// `AtomicPtr` keeps the read on the hot path down to a single load while still allowing
/// [`take`](Self::take) to hand ownership back on eject.
struct DetourSlot<T: Function>(AtomicPtr<GenericDetour<T>>);

impl<T: Function> DetourSlot<T> {
    const fn new() -> Self {
        Self(AtomicPtr::new(std::ptr::null_mut()))
    }

    /// The installed detour, or `None` before install and after teardown.
    fn get(&self) -> Option<&GenericDetour<T>> {
        // SAFETY: the pointer is null or a `Box` this slot owns. It is published with `Release`
        // before the detour it belongs to can be entered, and reclaimed only with every other thread
        // suspended, so a borrow taken here cannot outlive the allocation.
        unsafe { self.0.load(Ordering::Acquire).as_ref() }
    }

    /// Install `detour` into an empty slot. A second call leaves the slot alone and drops `detour`.
    fn set(&self, detour: GenericDetour<T>) {
        let raw = Box::into_raw(Box::new(detour));
        if self
            .0
            .compare_exchange(
                std::ptr::null_mut(),
                raw,
                Ordering::Release,
                Ordering::Relaxed,
            )
            .is_err()
        {
            // SAFETY: the slot was already occupied, so nothing else can have seen `raw`.
            drop(unsafe { Box::from_raw(raw) });
            // `set` is only called under `DETOUR_INSTALL`, so this should never fire; if it does, it
            // flags a real bug — a duplicate install attempted while the slot was already occupied.
            tracing::warn!("detour slot: duplicate install attempted and dropped");
        }
    }

    /// Empty the slot, returning the detour so dropping it frees the trampoline.
    fn take(&self) -> Option<Box<GenericDetour<T>>> {
        let raw = self.0.swap(std::ptr::null_mut(), Ordering::AcqRel);
        // SAFETY: a non-null pointer here is the `Box` this slot owned; the swap makes the take
        // exclusive.
        (!raw.is_null()).then(|| unsafe { Box::from_raw(raw) })
    }
}

unsafe extern "system" fn rs_set_scissor_rects_detour(
    context: *mut c_void,
    count: u32,
    rects: *const RECT,
) {
    let detour = RS_SET_SCISSOR_RECTS.get().expect("set before enable");
    if active() && count == 1 && !rects.is_null() {
        // Duplicating the single scissor into both slots unconditionally is correct because the
        // viewport detour keeps the scissor and viewport in lockstep:
        //
        // During a per-eye re-issue, both viewport slots are already pinned to the same eye half
        // (via `ensure_collapse_viewport` with `CollapseViewport::Eye`), so duplicating the scissor
        // into both slots matches the duplicated viewport.
        //
        // During the collapse split, both viewport slots are the two eye halves, and duplicating the
        // single scissor into both is the non-diverging fallback — each eye's scissor is the same
        // full-target rect. The scissor never needs to be split differently from the viewport because
        // the engine always sets them together.
        let rect = unsafe { *rects };
        unsafe { detour.call(context, 2, [rect, rect].as_ptr()) };
    } else {
        unsafe { detour.call(context, count, rects) };
    }
}

/// Log and reset the patched/unpatched draw counts of one G-buffer range -- called at each range's
/// exit, tagged with the `[first, last)` pass-index window the range covered so the frame's several
/// ranges can be told apart in the log.
///
/// `torn` counts the ranges whose guard found the flag already lowered: a non-zero value means the
/// range was closed from outside while it was still running, and every draw after that point was
/// mis-classified as out-of-range.
pub fn log_draw_split(first: u32, last: u32) {
    let patched = PATCHED_DRAWS.swap(0, Ordering::Relaxed);
    let unpatched = UNPATCHED_DRAWS.swap(0, Ordering::Relaxed);
    let split = VIEWPORT_SPLIT.swap(0, Ordering::Relaxed);
    let dup = VIEWPORT_DUP.swap(0, Ordering::Relaxed);
    let in_patched = INSTANCED_RANGE_PATCHED.swap(0, Ordering::Relaxed);
    let out_patched = INSTANCED_RANGE_OUT_PATCHED.swap(0, Ordering::Relaxed);
    let indirect_reissued = INDIRECT_REISSUED.swap(0, Ordering::Relaxed);
    let indirect_forwarded = INDIRECT_FORWARDED.swap(0, Ordering::Relaxed);
    let slot13 = drain_slot13_census();
    let torn = RANGE_TORN.swap(0, Ordering::Relaxed);
    let torn_total = RANGE_TORN_TOTAL.fetch_add(torn, Ordering::Relaxed) + torn;
    if torn > 0 {
        tracing::warn!(
            target: "single_pass",
            "pass range [{first:#x}, {last:#x}) was closed from outside while it ran: the draws after \
             that point were treated as out-of-range ({torn_total} so far this session)"
        );
    }
    if !diagnostic_frame() {
        return;
    }
    let s = substitution_stats();
    // Named per pass, since the whole point is to say *which* passes submit geometry this way.
    let slot13 = slot13
        .iter()
        .map(|(pass, count)| {
            let kind = if is_geometry_slot13_pass(*pass) {
                "geom"
            } else {
                "full"
            };
            format!("{pass:#04x}:{count}:{kind}")
        })
        .collect::<Vec<_>>()
        .join(" ");
    tracing::info!(
        target: "single_pass",
        "pass range [{first:#x}, {last:#x}): {patched} patched, {unpatched} unpatched draws | \
         instanced while it ran: {in_patched} patched in-range, {out_patched} patched out-of-range | \
         indirect: {indirect_reissued} re-issued per eye, {indirect_forwarded} forwarded | \
         torn {torn} ({torn_total} this session) | viewports: {split} split, {dup} identical-dup | \
         recorded VS={} | CreateVertexShader: pending={} reacq[patched={} cb13={} no-refs={} err={}] | \
         census[patched={} no-refs={} deferred={} errored={}] | slot-13 by pass: [{slot13}]",
        s.recorded_vs,
        s.cvs_pending,
        s.cvs_reacq_patched,
        s.cvs_reacq_cb13,
        s.cvs_reacq_no_refs,
        s.cvs_reacq_err,
        s.census_patched,
        s.census_no_refs,
        s.census_deferred,
        s.census_errored,
    );
}

/// Disable all the single-pass COM-vtable detours, restoring the original D3D functions. Must run on
/// eject **before** the payload unloads: the detours inline-patch the DXVK functions to jump into
/// payload code, so leaving them enabled while the DLL unmaps dangles those jumps and the next D3D
/// call crashes. Runs under a thread suspender and the install lock, like install -- with the same
/// caveat that suspension narrows rather than closes the in-prologue window.
pub fn uninstall_com_detours() {
    let _install = DETOUR_INSTALL.lock();
    if RS_SET_VIEWPORTS.get().is_none() {
        return; // never installed (single-pass never activated this session)
    }

    // Ordering here has to satisfy two constraints that pull against each other.
    //
    // The slots must stay populated for as long as the functions are still patched: a detour that
    // fires with its slot already emptied finds no trampoline and aborts (it runs in a `nounwind`
    // context, so the panic is fatal rather than recoverable).
    //
    // And the trampolines must be freed *outside* the thread suspender: dropping a detour frees its
    // trampoline, a heap free takes the process heap lock, and if a suspended thread holds that lock
    // the free waits on a thread only we can resume -- an unrecoverable wedge, seen as a hang with a
    // thread spinning in the game's `PlatformAllocHook`.
    //
    // So: disable under suspension with the slots intact, resume, and only then reclaim and drop.
    // Between the two, the functions are unpatched, so nothing can enter a detour at all.
    // Fixed-size and not a `Vec`: growing it would allocate, and allocation is exactly what must
    // not happen while other threads are suspended below. `disable_detour!` writes through a
    // checked accessor, so a slot count that falls behind the number of call sites drops the
    // overflowing name from the log instead of indexing out of bounds in this `nounwind` context.
    let mut failed: [Option<&'static str>; 10] = [None; 10];
    let mut failures = 0usize;

    let _ = ThreadSuspender::for_block(|| {
        // A detour left enabled here is a relay still pointing into the about-to-be-freed payload
        // image, so a swallowed failure would be an undiagnosable crash -- record it. Recorded
        // rather than logged because formatting a `tracing` event allocates, which is the very
        // thing that must not happen under suspension.
        macro_rules! disable_detour {
            ($slot:expr, $name:literal) => {
                if let Some(detour) = $slot.get() {
                    // SAFETY: patching the function back runs with all other threads suspended.
                    let bad = match unsafe { detour.disable() } {
                        Err(_) => true,
                        Ok(()) => detour.is_enabled(),
                    };
                    if bad {
                        // `failures` still increments past the end of the array so the count
                        // stays truthful even when a name gets dropped for lack of a slot.
                        if let Some(slot) = failed.get_mut(failures) {
                            *slot = Some($name);
                        }
                        failures += 1;
                    }
                }
            };
        }
        disable_detour!(RS_SET_VIEWPORTS, "RSSetViewports");
        disable_detour!(RS_SET_SCISSOR_RECTS, "RSSetScissorRects");
        disable_detour!(DRAW_INDEXED, "DrawIndexed");
        disable_detour!(DRAW, "Draw");
        disable_detour!(DRAW_INDEXED_INSTANCED, "DrawIndexedInstanced");
        disable_detour!(
            DRAW_INDEXED_INSTANCED_INDIRECT,
            "DrawIndexedInstancedIndirect"
        );
        disable_detour!(DRAW_INSTANCED_INDIRECT, "DrawInstancedIndirect");
        disable_detour!(VS_SET_SHADER, "VSSetShader");
        disable_detour!(CREATE_VERTEX_SHADER, "CreateVertexShader");
        disable_detour!(SET_VERTEX_PROGRAM_CONSTANTS, "SetVertexProgramConstants");
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    });

    for name in failed.iter().flatten() {
        tracing::error!(
            "single-pass: {name} did not disable (still dangling into the freed payload image)"
        );
    }

    // Threads are running again and the functions are unpatched, so these drops are unreachable by
    // any detour and their frees cannot deadlock against a suspended lock holder.
    drop(RS_SET_VIEWPORTS.take());
    drop(RS_SET_SCISSOR_RECTS.take());
    drop(DRAW_INDEXED.take());
    drop(DRAW.take());
    drop(DRAW_INDEXED_INSTANCED.take());
    drop(DRAW_INDEXED_INSTANCED_INDIRECT.take());
    drop(DRAW_INSTANCED_INDIRECT.take());
    drop(VS_SET_SHADER.take());
    drop(CREATE_VERTEX_SHADER.take());
    drop(SET_VERTEX_PROGRAM_CONSTANTS.take());
    release_cb13();
    for shader in std::mem::take(&mut *PATCHED_VS.lock()) {
        // SAFETY: every entry was `com_add_ref`'d exactly once when it was recorded.
        unsafe { com_release(shader as *mut c_void) };
    }
    PATCHED_VS_NAMES.lock().clear();
    // The transform decisions go with them. They are normally cleared by the eject's shader bounce,
    // but a session that patched nothing never bounces, and a `static` outlives the payload -- so a
    // re-inject with different flags could otherwise consult the previous session's decisions for
    // any blob the engine still has cached.
    VS_TRANSFORM_CACHE.lock().clear();
    reset_instanced_exposure();
    tracing::info!("single-pass: COM detours uninstalled");
}

/// Install the single-pass COM-vtable detours on the immediate-context (and device) vtables, once.
/// Patching runs under a thread suspender, which narrows the window in which another thread can be
/// executing a target's prologue while it is rewritten -- it does not close it: `SuspendThread` is
/// asynchronous, and no instruction pointer is inspected, so a thread already inside the bytes being
/// overwritten stays there. Called from the active render path and from the
/// `CreateVertexProgram` hook -- the latter so the `CreateVertexShader` detour that records a patched
/// shader into [`PATCHED_VS`] exists *before* the shader is created, not lazily on the first rendered
/// frame (a shader created in between, e.g. a character shader loaded at level start, would otherwise
/// be patched at the blob level but never recorded, so `BOUND_VS_PATCHED` stays false and its draw is
/// never doubled). A normal (single-pass-off) session never installs it.
pub(crate) fn ensure_viewport_detours() {
    // The whole body is serialized, not just the published-yet check: the callers are on different
    // threads (the render thread's cb13 mirror and the shader-creation thread's `CreateVertexProgram`),
    // and the publish happens only after seven `GenericDetour::new` calls. Two threads could otherwise
    // both pass an unpublished check and both reach `ThreadSuspender::for_block`, each suspending the
    // other -- a silent, permanent hang. `uninstall_com_detours` takes the same lock so an eject cannot
    // interleave with an install.
    let _install = DETOUR_INSTALL.lock();
    if RS_SET_VIEWPORTS.get().is_some() || crate::is_shutting_down() {
        return; // already installed, or tearing down -- never (re)install during eject
    }
    // SAFETY: reads the live immediate-context vtable; the two slots are the standard D3D11 layout.
    unsafe {
        let Some(ge) = GraphicsEngine::get() else {
            return;
        };
        let Some(device) = ge.m_Device.as_ref() else {
            return;
        };
        let Some(context) = device.m_Context.as_ref() else {
            return;
        };
        let vtable = *(context.m_Context.as_raw() as *const *const usize);
        let device_vtable = *(device.m_Device.as_raw() as *const *const usize);
        let viewports_target: RsSetViewportsFn =
            std::mem::transmute(*vtable.add(RS_SET_VIEWPORTS_SLOT));
        let scissors_target: RsSetScissorRectsFn =
            std::mem::transmute(*vtable.add(RS_SET_SCISSOR_RECTS_SLOT));
        let draw_indexed_target: DrawIndexedFn =
            std::mem::transmute(*vtable.add(DRAW_INDEXED_SLOT));
        let draw_target: DrawFn = std::mem::transmute(*vtable.add(DRAW_SLOT));
        let draw_indexed_instanced_target: DrawIndexedInstancedFn =
            std::mem::transmute(*vtable.add(DRAW_INDEXED_INSTANCED_SLOT));
        let draw_indexed_instanced_indirect_target: DrawIndirectFn =
            std::mem::transmute(*vtable.add(DRAW_INDEXED_INSTANCED_INDIRECT_SLOT));
        let draw_instanced_indirect_target: DrawIndirectFn =
            std::mem::transmute(*vtable.add(DRAW_INSTANCED_INDIRECT_SLOT));
        let vs_set_shader_target: VsSetShaderFn =
            std::mem::transmute(*vtable.add(VS_SET_SHADER_SLOT));
        let create_vertex_shader_target: CreateVertexShaderFn =
            std::mem::transmute(*device_vtable.add(CREATE_VERTEX_SHADER_SLOT));
        // Unlike the rest, this one is a static engine function (not a COM vtable slot): the leaf
        // vertex-constant stager, detoured so the baked-cb per-eye re-issue can reproject a block's own
        // constant upload.
        let set_vs_consts_target: SetVertexProgramConstantsFn =
            std::mem::transmute(jc3gi::graphics_engine::draw::SetVertexProgramConstants_ADDRESS);

        let (
            Ok(viewports_detour),
            Ok(scissors_detour),
            Ok(draw_indexed_detour_handle),
            Ok(draw_detour_handle),
            Ok(draw_indexed_instanced_detour_handle),
            Ok(draw_indexed_instanced_indirect_detour_handle),
            Ok(draw_instanced_indirect_detour_handle),
            Ok(vs_set_shader_detour_handle),
            Ok(create_vertex_shader_detour_handle),
            Ok(set_vs_consts_detour_handle),
        ) = (
            GenericDetour::new(viewports_target, rs_set_viewports_detour),
            GenericDetour::new(scissors_target, rs_set_scissor_rects_detour),
            GenericDetour::new(draw_indexed_target, draw_indexed_detour),
            GenericDetour::new(draw_target, draw_detour),
            GenericDetour::new(draw_indexed_instanced_target, draw_indexed_instanced_detour),
            GenericDetour::new(
                draw_indexed_instanced_indirect_target,
                draw_indexed_instanced_indirect_detour,
            ),
            GenericDetour::new(
                draw_instanced_indirect_target,
                draw_instanced_indirect_detour,
            ),
            GenericDetour::new(vs_set_shader_target, vs_set_shader_detour),
            GenericDetour::new(create_vertex_shader_target, create_vertex_shader_detour),
            GenericDetour::new(set_vs_consts_target, set_vertex_program_constants_detour),
        )
        else {
            tracing::warn!("single-pass: COM detour construction failed");
            return;
        };

        // Publish into the statics before enabling, so a detour that fires mid-enable finds its
        // trampoline. Enabling itself runs with other threads suspended.
        RS_SET_VIEWPORTS.set(viewports_detour);
        RS_SET_SCISSOR_RECTS.set(scissors_detour);
        DRAW_INDEXED.set(draw_indexed_detour_handle);
        DRAW.set(draw_detour_handle);
        DRAW_INDEXED_INSTANCED.set(draw_indexed_instanced_detour_handle);
        DRAW_INDEXED_INSTANCED_INDIRECT.set(draw_indexed_instanced_indirect_detour_handle);
        DRAW_INSTANCED_INDIRECT.set(draw_instanced_indirect_detour_handle);
        VS_SET_SHADER.set(vs_set_shader_detour_handle);
        CREATE_VERTEX_SHADER.set(create_vertex_shader_detour_handle);
        SET_VERTEX_PROGRAM_CONSTANTS.set(set_vs_consts_detour_handle);
        let _ = ThreadSuspender::for_block(|| {
            RS_SET_VIEWPORTS.get().expect("just set").enable().ok();
            RS_SET_SCISSOR_RECTS.get().expect("just set").enable().ok();
            DRAW_INDEXED.get().expect("just set").enable().ok();
            DRAW.get().expect("just set").enable().ok();
            DRAW_INDEXED_INSTANCED
                .get()
                .expect("just set")
                .enable()
                .ok();
            DRAW_INDEXED_INSTANCED_INDIRECT
                .get()
                .expect("just set")
                .enable()
                .ok();
            DRAW_INSTANCED_INDIRECT
                .get()
                .expect("just set")
                .enable()
                .ok();
            VS_SET_SHADER.get().expect("just set").enable().ok();
            CREATE_VERTEX_SHADER.get().expect("just set").enable().ok();
            SET_VERTEX_PROGRAM_CONSTANTS
                .get()
                .expect("just set")
                .enable()
                .ok();
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        });
        tracing::info!("single-pass: viewport + draw + shader-tracking COM detours installed");
    }
}

/// Serializes [`ensure_viewport_detours`] against itself and against [`uninstall_com_detours`]. Both
/// suspend every other thread while they patch, so two of them running concurrently would suspend each
/// other and hang the process.
static DETOUR_INSTALL: Mutex<()> = Mutex::new(());

static IN_GBUFFER_RANGE: AtomicBool = AtomicBool::new(false);
/// Real frames since injection, advanced by [`begin_frame`]; the diagnostics' cadence and grouping.
static FRAME_ORDINAL: AtomicUsize = AtomicUsize::new(0);
/// Ranges closed from outside their guard since the last [`log_draw_split`], and the session total --
/// see [`GBufferRange::drop`]. The tear is intermittent, so the total is never reset: a diagnostic
/// frame that reports zero of its own still shows whether it has ever happened.
static RANGE_TORN: AtomicUsize = AtomicUsize::new(0);
static RANGE_TORN_TOTAL: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// Set by the `CreateVertexProgram` hook right before the engine creates the D3D shader from a
    /// substituted (patched) blob, so [`create_vertex_shader_detour`] knows the next shader is patched.
    ///
    /// Thread-local, not a global: `ID3D11Device` is free-threaded and JC3 streams resources off the
    /// render thread, so a loader thread's flag would otherwise tag whatever shader a *different*
    /// thread happened to create next -- instance-doubling and eye-splitting an unrelated shader while
    /// the genuinely patched one went unrecorded. The hook sets it and the device detour consumes it
    /// within the same synchronous call, so the flag never needs to cross a thread.
    static PATCH_PENDING: Cell<bool> = const { Cell::new(false) };

    /// The engine name of the shader [`PATCH_PENDING`] refers to, carried alongside it so
    /// [`create_vertex_shader_detour`] can record the name against the `ID3D11VertexShader` it gets
    /// back. The D3D layer never sees the name, and the shader pointer is the only identity the
    /// draw-time paths have, so this is the one point where the two can be joined.
    static PATCH_PENDING_NAME: Cell<Option<String>> = const { Cell::new(None) };
}

/// Set (or clear) this thread's [`PATCH_PENDING`] flag and the pending shader's engine name. Called
/// by the `CreateVertexProgram` hook around the engine's shader creation.
pub fn set_patch_pending(pending: bool, name: Option<&str>) {
    PATCH_PENDING.with(|flag| flag.set(pending));
    PATCH_PENDING_NAME.with(|slot| slot.set(pending.then(|| name.map(str::to_owned)).flatten()));
}

/// The engine name of each recorded patched vertex shader, where one was available (the
/// `CreateVertexProgram` path carries it; the re-acquire path does not). Read only by the diagnostic
/// readouts, never on a draw path.
static PATCHED_VS_NAMES: Mutex<BTreeMap<usize, String>> = Mutex::new(BTreeMap::new());
/// The `ID3D11VertexShader`s created from patched blobs, keyed by their raw pointer.
///
/// The set *owns* a reference to each shader: [`com_add_ref`] on record, [`com_release`] on
/// [`reset_patched_vs`]. Without that reference a shader the game releases could have its address
/// recycled by an unpatched shader, which would then match on `VSSetShader` and be instance-doubled --
/// the recycled draw appears in one eye only. An ordered set rather than a linear scan because the
/// lookup is on the hottest path in the codebase: every `VSSetShader` of every frame, in a feature
/// whose whole purpose is cutting draw-submission cost. (The raw pointer is stored rather than an
/// owned `IUnknown` only because `IUnknown` is not `Send`.)
static PATCHED_VS: Mutex<BTreeSet<usize>> = Mutex::new(BTreeSet::new());

/// Take a reference of our own on a COM object, so the pointer we record cannot be freed (and its
/// address recycled) while we still consider it live. Paired with [`com_release`].
///
/// # Safety
///
/// `object` must be a live COM object pointer.
unsafe fn com_add_ref(object: *mut c_void) {
    // SAFETY: the caller guarantees a live object; cloning the borrowed interface calls `AddRef`, and
    // forgetting the clone is what keeps that reference outstanding.
    if let Some(unknown) = unsafe { IUnknown::from_raw_borrowed(&object) } {
        std::mem::forget(unknown.clone());
    }
}

/// Drop the reference [`com_add_ref`] took.
///
/// # Safety
///
/// `object` must be a pointer this module previously passed to [`com_add_ref`], not yet released.
unsafe fn com_release(object: *mut c_void) {
    // SAFETY: `from_raw` adopts the outstanding reference; dropping it calls `Release`.
    drop(unsafe { IUnknown::from_raw(object) });
}
/// Whether the currently-bound vertex shader is a patched one (updated on `VSSetShader`).
static BOUND_VS_PATCHED: AtomicBool = AtomicBool::new(false);
/// The currently-bound `ID3D11VertexShader` pointer (updated on `VSSetShader`), so an exposed
/// already-instanced draw can be attributed to a shader.
static BOUND_VS: AtomicUsize = AtomicUsize::new(0);
static PATCHED_DRAWS: AtomicUsize = AtomicUsize::new(0);
static UNPATCHED_DRAWS: AtomicUsize = AtomicUsize::new(0);
/// Already-instanced draws with a patched vertex shader bound, split by whether the G-buffer range
/// was up, accumulated per *range* rather than per frame ([`log_draw_split`] resets them). The
/// per-frame `INSTANCED_*` buckets carry the same events; these say *when in the frame* they landed.
static INSTANCED_RANGE_PATCHED: AtomicUsize = AtomicUsize::new(0);
static INSTANCED_RANGE_OUT_PATCHED: AtomicUsize = AtomicUsize::new(0);
static VIEWPORT_SPLIT: AtomicUsize = AtomicUsize::new(0);
static VIEWPORT_DUP: AtomicUsize = AtomicUsize::new(0);
/// How often [`unify_viewport_slots`] had to put a split slot pair back to one region: the number of
/// windows in which an out-of-range patched draw would otherwise have lost its odd-parity instances.
static VIEWPORT_UNIFIED: AtomicUsize = AtomicUsize::new(0);

/// `CreateVertexShader`-detour outcome tallies (cumulative since injection), to diagnose what the
/// shader re-create path -- which bypasses `CreateVertexProgram` -- feeds through the D3D-level
/// substitution: `pending` came pre-substituted from `CreateVertexProgram`; the two `decided_*` buckets
/// re-applied the decision the hook recorded for that blob; the four `reacq_*` buckets are what the
/// detour's own rewrite found for a blob the hook never saw.
static CVS_PENDING: AtomicUsize = AtomicUsize::new(0);
/// A blob the hook had decided a transform for, re-applied here.
static CVS_DECIDED_TRANSFORMED: AtomicUsize = AtomicUsize::new(0);
/// A blob the hook had decided to leave pristine -- overwhelmingly the families a render-block
/// intercept owns. A count here is the intercepts and the rewrite staying out of each other's way; the
/// same shaders showing up under `reacq_patched` instead would mean the decline was being undone.
static CVS_DECIDED_PRISTINE: AtomicUsize = AtomicUsize::new(0);
static CVS_REACQ_PATCHED: AtomicUsize = AtomicUsize::new(0);
/// The incoming bytecode already declared `cb13`, so the rewriter refused it as already-patched.
///
/// No pristine game vertex shader declares `cb13` -- the offline corpus run over all 455 of them
/// reports zero `Cb13AlreadyDeclared`, which is why the register was chosen -- so a non-zero count is
/// unambiguous: something is presenting the mod's *own* rewritten bytecode back to
/// `CreateVertexShader`, from a store the substitution paths do not own (both of them repoint the
/// engine's code pointer only for the duration of the create call and restore it afterwards).
///
/// That matters for eject. The restore bounce re-creates every shader from whatever bytecode its
/// resource holds, with the substitution inert; a resource holding patched bytecode therefore
/// re-creates a patched shader, and there is no inverse rewrite to undo it. Which store that is has
/// not been identified -- it needs a live session with a non-zero count and a breakpoint on the
/// caller. [`warn_if_shaders_hold_patched_bytecode`] makes it visible at eject rather than silent.
static CVS_REACQ_CB13: AtomicUsize = AtomicUsize::new(0);
static CVS_REACQ_NOREFS: AtomicUsize = AtomicUsize::new(0);
static CVS_REACQ_ERR: AtomicUsize = AtomicUsize::new(0);

/// The last full (unsplit) viewport bound during a collapsed camera scene, recorded by
/// [`rs_set_viewports_detour`] so [`ensure_collapse_viewport`] can derive the L/R eye halves at draw
/// time. `None` until the scene's first viewport bind.
///
/// Deliberately *not* cleared at the end of the G-buffer range, unlike [`CURRENT_M_EYE`]: the
/// post-draw UI overlay ([`collapse_ui_eye_override`]) reads it to place each eye's HUD quad, and that
/// runs long after the range. Every consumer that must not see a stale value gates on
/// [`in_gbuffer_range`] instead.
static COLLAPSE_FULL_VIEWPORT: Mutex<Option<D3D11_VIEWPORT>> = Mutex::new(None);

/// The viewport the **engine** last bound during a collapsed camera scene, whatever its size, recorded
/// by [`rs_set_viewports_detour`].
///
/// [`COLLAPSE_FULL_VIEWPORT`] deliberately records only *scene-sized* binds, because the eye halves
/// were derived from it and following a half-resolution post target would have mis-split the scene.
/// That is right for the scene notion and wrong for the split: several passes redirect their draws to
/// a reduced-resolution off-screen target -- the shared quarter-resolution buffer the low-resolution
/// clouds, the low-resolution particles, and the volumetric spot-light cones all render into, and the
/// downsampled depth buffer -- and a draw into a `W x H/2` target handed a `2W x H` viewport is
/// magnified 2x about the target's origin and cropped, which is a 2x motion gain as well.
///
/// So this record exists alongside rather than replacing: it is always the live bind, so it cannot go
/// stale the way a single shared record would, and the halves derived from it are the halves of
/// whatever target is actually bound.
static CURRENT_ENGINE_VIEWPORT: Mutex<Option<D3D11_VIEWPORT>> = Mutex::new(None);
/// The two per-eye reprojection matrices `M_eye` (`clip_eye = M_eye · clip_center`), published each
/// view by [`compute_dual_eye_rows`]. The terrain-detail render-block intercept reads them to build a
/// per-eye `cb1` on the CPU (the detail draw is GPU-indirect, so it cannot be instance-doubled).
static CURRENT_M_EYE: Mutex<Option<[glam::Mat4; 2]>> = Mutex::new(None);

static CAPABILITY: AtomicU8 = AtomicU8::new(Capability::Unprobed as u8);
static PATCHED: AtomicUsize = AtomicUsize::new(0);
static NO_REFS: AtomicUsize = AtomicUsize::new(0);
static DEFERRED: AtomicUsize = AtomicUsize::new(0);
static ERRORED: AtomicUsize = AtomicUsize::new(0);
/// Terrain tessellation shaders substituted for single-pass since injection: hull shaders whose eye
/// lane was forwarded, and domain shaders reprojected. Surfaced in the debug UI so it is clear whether
/// the terrain path is catching anything.
static TERRAIN_HS_FORWARDED: AtomicUsize = AtomicUsize::new(0);
static TERRAIN_DS_REPROJECTED: AtomicUsize = AtomicUsize::new(0);
