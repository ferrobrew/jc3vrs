//! Single-pass stereo (experimental): render the G-buffer geometry once, emitting both eyes via
//! instancing + `SV_ViewportArrayIndex` routing into a double-wide render target, instead of the
//! double-draw (two full `game.Draw` walks, one per eye). See `docs/mod/single-pass-stereo.md` for
//! the design and phased plan.
//!
//! This module owns the mod-side state that the double-draw path does not need:
//! - the DXVK viewport-routing **capability probe** ([`probe`] / [`capability`]);
//! - the vertex-shader rewrite **census** ([`record_patch_outcome`] and the `*_count` getters), which
//!   the `CreateVertexProgram` hook feeds so the debug UI can report how the rewriter fared against
//!   the game's real shader set.
//!
//! The rest of the pipeline (cb13 dual-eye upload, the double-wide render-setup re-init, the
//! draw-doubling) is built out under [`crate::config::StereoConfig::single_pass`]; until it lands,
//! [`crate::config::StereoConfig::single_pass_patch_dryrun`] runs the census with no rendering change.

use std::{
    ffi::c_void,
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
    },
};

use dxbc_stereo::DxbcError;
use jc3gi::{
    graphics_engine::{graphics_engine::GraphicsEngine, render_engine::RenderEngine},
    types::math::{Matrix4, Vector4},
};
use parking_lot::Mutex;
use re_utilities::ThreadSuspender;
use retour::GenericDetour;
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
    core::Interface,
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

/// Record the outcome of running [`dxbc_stereo::patch_vertex_shader`] on one vertex shader, for the
/// census the debug UI reports. Classifies into four buckets: successfully patched; no per-eye
/// references (the baked-WVP / no-position families left double-drawn -- expected); the
/// `SV_InstanceID`-already-declared deferral (shaders that instance themselves, whose `>> 1` consumer
/// rewrite is a later phase -- also expected, left double-drawn); and genuinely errored (an
/// unexpected shape the rewriter could not handle -- worth investigating, should be zero).
pub fn record_patch_outcome(outcome: &Result<Vec<u8>, DxbcError>) {
    match outcome {
        Ok(_) => &PATCHED,
        Err(DxbcError::NoPerEyeReferences) => &NO_REFS,
        Err(DxbcError::InstanceIdAlreadyDeclared) => &DEFERRED,
        Err(_) => &ERRORED,
    }
    .fetch_add(1, Ordering::Relaxed);
}

/// Vertex shaders successfully rewritten for single-pass since injection.
pub fn patched_count() -> usize {
    PATCHED.load(Ordering::Relaxed)
}

/// Vertex shaders with no per-eye `cb0` references -- the baked-WVP / no-position families left
/// double-drawn. Expected, not a failure.
pub fn no_refs_count() -> usize {
    NO_REFS.load(Ordering::Relaxed)
}

/// Vertex shaders left double-drawn because they already declare an `SV_InstanceID` input; their
/// `>> 1` consumer rewrite is a later phase. Expected, not a failure.
pub fn deferred_count() -> usize {
    DEFERRED.load(Ordering::Relaxed)
}

/// Vertex shaders the rewriter could not handle for an unexpected reason (a shape it does not yet
/// support). A non-zero count flags shaders to investigate -- the offline corpus reports zero.
pub fn errored_count() -> usize {
    ERRORED.load(Ordering::Relaxed)
}

/// Reset the census counters (on a shader reload, so the reported numbers reflect one clean pass over
/// the shader set rather than accumulating across reloads).
pub fn reset_census() {
    PATCHED.store(0, Ordering::Relaxed);
    NO_REFS.store(0, Ordering::Relaxed);
    DEFERRED.store(0, Ordering::Relaxed);
    ERRORED.store(0, Ordering::Relaxed);
}

/// Whether single-pass rendering should actually run this frame: the master switch is on, the
/// census-only dry-run is off, and the device supports viewport routing. The VS-substitution and
/// cb13 paths gate on this; when it is false the double-draw path is left untouched.
pub fn active() -> bool {
    let (single_pass, dry_run) =
        Config::lock_query(|c| (c.stereo.single_pass, c.stereo.single_pass_patch_dryrun));
    single_pass && !dry_run && capability() == Capability::Supported
}

/// Whether the Milestone B "make the eyes diverge" step is on (in addition to [`active`]): distinct
/// per-eye `cb13`, left/right-half viewport routing, and instance doubling of the G-buffer geometry.
pub fn dual_eye_active() -> bool {
    let (single_pass, dry_run, dual_eye) = Config::lock_query(|c| {
        (
            c.stereo.single_pass,
            c.stereo.single_pass_patch_dryrun,
            c.stereo.single_pass_dual_eye,
        )
    });
    single_pass && !dry_run && dual_eye && capability() == Capability::Supported
}

/// Marks whether the render thread is currently inside the G-buffer geometry pass range
/// (`RP_Z_OCCLUDERS..RP_FIRST_SCENE`), set around that `DrawRenderPassRange` call. The dual-eye
/// viewport split and instance doubling apply only here -- so shadow/lighting/post passes, which
/// reuse the same patched shaders but are not double-wide, keep the identical-viewport behaviour.
pub fn set_gbuffer_range(inside: bool) {
    IN_GBUFFER_RANGE.store(inside, Ordering::Relaxed);
}

fn in_gbuffer_range() -> bool {
    IN_GBUFFER_RANGE.load(Ordering::Relaxed)
}

/// The stereo constant buffer's register slot (`b13`, free across the game's vertex shaders) and its
/// size in float4 rows (five per eye: four view-projection rows then the camera position, two eyes).
const STEREO_CB_REGISTER: u32 = 13;
const STEREO_CB_ROWS: usize = 10;

/// The `cb0` (`m_VPGlobalConstData`) rows the patched shaders read per eye, in the order the rewrite
/// lays them out in `cb13`: the four translation-free view-projection rows (`cb0[29..32]`), then the
/// camera world position (`cb0[4]`). See `dxbc_stereo::PER_EYE_CB0_ROWS`.
const PER_EYE_SOURCE_ROWS: [usize; 5] = [29, 30, 31, 32, 4];

/// Mirror the current view's per-eye `cb0` rows into the mod-owned `cb13` and bind it at `b13`.
///
/// Milestone A of the single-pass build: both eye slots get the **same** (current-view) rows, so a
/// patched vertex shader -- which reads its position from `cb13` instead of `cb0` -- renders exactly
/// what it would have from `cb0`, in *every* pass (the G-buffer, but also the shadow and reflection
/// passes that reuse the same model shaders under a different view). That shadow-safety is why `cb13`
/// tracks whatever view is current rather than being written once. Later milestones diverge the two
/// slots (eye 0 / eye 1) for the doubled G-buffer draw.
///
/// Called from the `SetAllGlobalShaderProgramConstants` detour, after the engine has refreshed
/// `m_VPGlobalConstData` and uploaded `cb0`, on the render thread.
pub fn mirror_and_bind_cb13(engine: &RenderEngine) {
    // Ensure the viewport-duplication detours are installed (once, on the first active frame).
    ensure_viewport_detours();

    // Dual-eye (Milestone B): during the main-scene G-buffer range, fill the two eye slots with
    // *distinct* per-eye view-projections so the eyes diverge. Everywhere else (shadow/reflection
    // passes, and Milestone A) mirror the current view into both slots -- diverging those would be
    // wrong (they render from the sun/reflection camera, not the eye camera).
    let rows = if dual_eye_active() && in_gbuffer_range() {
        compute_dual_eye_rows(engine).unwrap_or_else(|| mirror_rows(engine))
    } else {
        mirror_rows(engine)
    };

    // SAFETY: `GraphicsEngine::get` returns the live singleton or `None`; the device/context pointers
    // are stable once the engine has initialised, and the ops run under the engine's context mutex.
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
        EnterCriticalSection(context.m_Mutex);
        let result = CB13
            .lock()
            .upload_and_bind(&device.m_Device, &context.m_Context, &rows);
        LeaveCriticalSection(context.m_Mutex);
        if let Err(e) = result {
            tracing::warn!("single-pass cb13: {e}");
        }
    }
}

/// Mirror the current view's per-eye `cb0` rows into both `cb13` eye slots (Milestone A / non-scene
/// passes): a patched shader then renders exactly what it would from `cb0`.
fn mirror_rows(engine: &RenderEngine) -> [Vector4; STEREO_CB_ROWS] {
    let vp = &engine.m_VPGlobalConstData;
    let mut rows = [Vector4::default(); STEREO_CB_ROWS];
    for eye in 0..2 {
        for (k, &src) in PER_EYE_SOURCE_ROWS.iter().enumerate() {
            rows[eye * PER_EYE_SOURCE_ROWS.len() + k] = vp[src];
        }
    }
    rows
}

/// Compute distinct per-eye `cb13` rows from the pristine center render-camera transform and the
/// per-eye [`EyeRenderParams`](crate::vr::frame::EyeRenderParams), replicating the double-draw's
/// per-eye camera math (`hooks/camera.rs`) purely in mod code -- so the single walk produces both
/// eyes. Returns `None` (falling back to the mirror) if the center transform or per-eye params are
/// not available this frame.
///
/// Per eye: offset the center world transform by the eye parallax + orientation delta, invert to a
/// view, zero its translation for the camera-relative OffsetVP, multiply by the reverse-Z eye
/// projection, and pair it with the eye's camera world position (`center campos + world_offset`).
/// The engine `Matrix4` <-> `glam::Mat4` bridge is a transpose, so the math is done in glam
/// column-vector form and converted back once (see the `Matrix4` doc-comment).
fn compute_dual_eye_rows(engine: &RenderEngine) -> Option<[Vector4; STEREO_CB_ROWS]> {
    let center_transform = crate::stereo::STEREO_STATE.lock().center_transform?;
    let center_world = glam::Mat4::from(center_transform);
    let center_campos = engine.m_VPGlobalConstData[4];

    let mut rows = [Vector4::default(); STEREO_CB_ROWS];
    for eye in 0..2 {
        let params = crate::vr::render_params(eye)?;

        let mut eye_world = center_world;
        eye_world.w_axis += params.world_offset.extend(0.0);
        let eye_world = eye_world * glam::Mat4::from_quat(params.orientation_delta);

        let mut offset_view = eye_world.inverse();
        offset_view.w_axis = glam::Vec4::new(0.0, 0.0, 0.0, 1.0);

        let offset_vp = glam::Mat4::from(params.projection_reverse_z) * offset_view;
        let offset_vp = Matrix4::from(offset_vp);

        for r in 0..4 {
            rows[eye * 5 + r] = Vector4 {
                data: [
                    offset_vp.data[r * 4],
                    offset_vp.data[r * 4 + 1],
                    offset_vp.data[r * 4 + 2],
                    offset_vp.data[r * 4 + 3],
                ],
            };
        }
        rows[eye * 5 + 4] = Vector4 {
            data: [
                center_campos.data[0] + params.world_offset.x,
                center_campos.data[1] + params.world_offset.y,
                center_campos.data[2] + params.world_offset.z,
                center_campos.data[3],
            ],
        };
    }
    Some(rows)
}

/// The mod-owned `cb13` constant buffer, lazily created and updated per view.
struct Cb13Buffer {
    buffer: Option<ID3D11Buffer>,
}

impl Cb13Buffer {
    /// Ensure the dynamic `cb13` buffer exists, write `rows` into it, and bind it at `b13`.
    unsafe fn upload_and_bind(
        &mut self,
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        rows: &[Vector4; STEREO_CB_ROWS],
    ) -> Result<(), windows::core::Error> {
        let byte_width = std::mem::size_of_val(rows) as u32;
        let buffer = match &self.buffer {
            Some(buffer) => buffer,
            None => {
                let mut created = None;
                unsafe {
                    device.CreateBuffer(
                        &D3D11_BUFFER_DESC {
                            ByteWidth: byte_width,
                            Usage: D3D11_USAGE_DYNAMIC,
                            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                            ..Default::default()
                        },
                        Some(&D3D11_SUBRESOURCE_DATA {
                            pSysMem: rows.as_ptr().cast(),
                            ..Default::default()
                        }),
                        Some(&mut created),
                    )?;
                }
                self.buffer
                    .insert(created.expect("CreateBuffer returned Ok with no buffer"))
            }
        };

        unsafe {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))?;
            std::ptr::copy_nonoverlapping(rows.as_ptr(), mapped.pData.cast(), STEREO_CB_ROWS);
            context.Unmap(buffer, 0);
            context.VSSetConstantBuffers(STEREO_CB_REGISTER, Some(&[Some(buffer.clone())]));
        }
        Ok(())
    }
}

/// If single-pass is active, duplicate the immediate context's current viewport (and scissor) into
/// slot 1. Called right after the engine binds a render setup ([`SetRenderSetup`]), which is where
/// the viewport is (re)set -- including per-cascade in the shadow passes, so slot 1 tracks whatever
/// region is currently bound rather than going stale between binds.
pub fn duplicate_current_viewport() {
    if !active() {
        return;
    }
    // SAFETY: runs on the render thread after a render-setup bind; the device/context pointers are
    // stable and the ops run under the engine's context mutex.
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
        EnterCriticalSection(context.m_Mutex);
        duplicate_viewport(&context.m_Context);
        LeaveCriticalSection(context.m_Mutex);
    }
}

/// Duplicate the current (single) viewport into viewport slots 0 **and** 1, both covering the same
/// region.
///
/// A patched shader writes `SV_ViewportArrayIndex = SV_InstanceID & 1`. Milestone A does not double
/// instances or set up per-eye viewports, so an instanced draw's odd-`SV_InstanceID` primitives would
/// route to viewport 1 -- which the engine never bound -- and be discarded, dropping half of every
/// instanced object (the flicker, since VR head-motion re-sorts which instance ids are odd). Binding
/// a second, identical viewport makes index 1 valid and render the same as index 0. (Milestone B
/// replaces the two identical viewports with the left/right halves of the double-wide target.)
unsafe fn duplicate_viewport(context: &ID3D11DeviceContext) {
    unsafe {
        let mut count = 1u32;
        let mut viewports = [D3D11_VIEWPORT::default(); 1];
        context.RSGetViewports(&mut count, Some(viewports.as_mut_ptr()));
        // Only duplicate a real viewport; a zero-width one (no viewport bound yet) would clip
        // everything to nothing.
        if viewports[0].Width > 0.0 {
            context.RSSetViewports(Some(&[viewports[0], viewports[0]]));
        }

        // If scissor testing is on, viewport 1 pairs with scissor rect 1; duplicate the engine's
        // rect into slot 1 too, else index-1 primitives clip to an empty (unset) rect.
        let mut scissor_count = 1u32;
        let mut scissors = [RECT::default(); 1];
        context.RSGetScissorRects(&mut scissor_count, Some(scissors.as_mut_ptr()));
        if scissors[0].right > scissors[0].left && scissors[0].bottom > scissors[0].top {
            context.RSSetScissorRects(Some(&[scissors[0], scissors[0]]));
        }
    }
}

// The mirror at `SetRenderSetup` (above) covers the scene passes, but the shadow cascades set their
// viewport through a raw `RSSetViewports` between binds, which that hook does not see -- so slot 1
// goes stale there and odd-instance shadow casters route to the wrong region (flickering shadows).
// Detouring `RSSetViewports`/`RSSetScissorRects` on the immediate-context vtable catches *every*
// viewport set, wherever it comes from, and mirrors a single-viewport set into two identical slots.

/// `ID3D11DeviceContext` vtable slots (7 base `IUnknown`/`ID3D11DeviceChild` slots + the method's
/// index), verified against `windows`'s `ID3D11DeviceContext_Vtbl`.
const RS_SET_VIEWPORTS_SLOT: usize = 44;
const RS_SET_SCISSOR_RECTS_SLOT: usize = 45;

type RsSetViewportsFn = unsafe extern "system" fn(*mut c_void, u32, *const D3D11_VIEWPORT);
type RsSetScissorRectsFn = unsafe extern "system" fn(*mut c_void, u32, *const RECT);

static RS_SET_VIEWPORTS: OnceLock<GenericDetour<RsSetViewportsFn>> = OnceLock::new();
static RS_SET_SCISSOR_RECTS: OnceLock<GenericDetour<RsSetScissorRectsFn>> = OnceLock::new();

unsafe extern "system" fn rs_set_viewports_detour(
    context: *mut c_void,
    count: u32,
    viewports: *const D3D11_VIEWPORT,
) {
    let detour = RS_SET_VIEWPORTS.get().expect("set before enable");
    if active() && count == 1 && !viewports.is_null() {
        let vp = unsafe { *viewports };
        let (slot0, slot1) = if dual_eye_active() && in_gbuffer_range() {
            // Route the two eyes to the left/right halves of the (double-wide) target.
            let half = vp.Width / 2.0;
            let mut left = vp;
            left.Width = half;
            let mut right = vp;
            right.Width = half;
            right.TopLeftX = vp.TopLeftX + half;
            (left, right)
        } else {
            // Milestone A: both slots identical, so a patched shader routes anywhere validly.
            (vp, vp)
        };
        unsafe { detour.call(context, 2, [slot0, slot1].as_ptr()) };
    } else {
        unsafe { detour.call(context, count, viewports) };
    }
}

unsafe extern "system" fn rs_set_scissor_rects_detour(
    context: *mut c_void,
    count: u32,
    rects: *const RECT,
) {
    let detour = RS_SET_SCISSOR_RECTS.get().expect("set before enable");
    if active() && count == 1 && !rects.is_null() {
        let rect = unsafe { *rects };
        unsafe { detour.call(context, 2, [rect, rect].as_ptr()) };
    } else {
        unsafe { detour.call(context, count, rects) };
    }
}

/// `ID3D11DeviceContext` vtable slots for the two indexed-draw entry points (verified against
/// `windows`'s `ID3D11DeviceContext_Vtbl`: field 6 → slot 12, field 14 → slot 20).
const DRAW_INDEXED_SLOT: usize = 12;
const DRAW_INDEXED_INSTANCED_SLOT: usize = 20;

type DrawIndexedFn = unsafe extern "system" fn(*mut c_void, u32, u32, i32);
type DrawIndexedInstancedFn = unsafe extern "system" fn(*mut c_void, u32, u32, u32, i32, u32);

static DRAW_INDEXED: OnceLock<GenericDetour<DrawIndexedFn>> = OnceLock::new();
/// The raw `DrawIndexedInstanced` entry (not detoured), used to re-issue a promoted draw.
static DRAW_INDEXED_INSTANCED_RAW: OnceLock<DrawIndexedInstancedFn> = OnceLock::new();

/// Promote a non-instanced `DrawIndexed` into a 2-instance `DrawIndexedInstanced` while the dual-eye
/// G-buffer geometry is drawing, so the patched shader's `SV_InstanceID & 1` selects the eye and
/// `SV_ViewportArrayIndex` routes it to that eye's viewport half. Already-instanced draws
/// (`DrawIndexedInstanced`) are left alone for now -- doubling those would need per-instance-buffer
/// step handling, so their geometry stays single-eye until a later step.
unsafe extern "system" fn draw_indexed_detour(
    context: *mut c_void,
    index_count: u32,
    start_index: u32,
    base_vertex: i32,
) {
    let detour = DRAW_INDEXED.get().expect("set before enable");
    if dual_eye_active()
        && in_gbuffer_range()
        && let Some(instanced) = DRAW_INDEXED_INSTANCED_RAW.get()
    {
        unsafe { instanced(context, index_count, 2, start_index, base_vertex, 0) };
    } else {
        unsafe { detour.call(context, index_count, start_index, base_vertex) };
    }
}

/// Install the `RSSetViewports`/`RSSetScissorRects` duplication detours on the immediate-context
/// vtable, once. Patching runs under a thread suspender (all other threads paused) so none can be
/// executing the target's prologue while it is rewritten. Called only from the active render path, so
/// a normal (single-pass-off) session never installs it.
fn ensure_viewport_detours() {
    if RS_SET_VIEWPORTS.get().is_some() {
        return;
    }
    // SAFETY: reads the live immediate-context vtable; the two slots are the standard D3D11 layout,
    // and the detour targets are enabled under a thread suspender.
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
        let viewports_target: RsSetViewportsFn =
            std::mem::transmute(*vtable.add(RS_SET_VIEWPORTS_SLOT));
        let scissors_target: RsSetScissorRectsFn =
            std::mem::transmute(*vtable.add(RS_SET_SCISSOR_RECTS_SLOT));
        let draw_indexed_target: DrawIndexedFn =
            std::mem::transmute(*vtable.add(DRAW_INDEXED_SLOT));
        let draw_indexed_instanced: DrawIndexedInstancedFn =
            std::mem::transmute(*vtable.add(DRAW_INDEXED_INSTANCED_SLOT));

        let Ok(viewports_detour) = GenericDetour::new(viewports_target, rs_set_viewports_detour)
        else {
            tracing::warn!("single-pass: RSSetViewports detour construction failed");
            return;
        };
        let Ok(scissors_detour) = GenericDetour::new(scissors_target, rs_set_scissor_rects_detour)
        else {
            tracing::warn!("single-pass: RSSetScissorRects detour construction failed");
            return;
        };
        let Ok(draw_indexed_detour_handle) =
            GenericDetour::new(draw_indexed_target, draw_indexed_detour)
        else {
            tracing::warn!("single-pass: DrawIndexed detour construction failed");
            return;
        };

        // Publish into the statics before enabling, so a detour that fires mid-enable finds its
        // trampoline. Enabling itself runs with other threads suspended.
        let _ = DRAW_INDEXED_INSTANCED_RAW.set(draw_indexed_instanced);
        let _ = RS_SET_VIEWPORTS.set(viewports_detour);
        let _ = RS_SET_SCISSOR_RECTS.set(scissors_detour);
        let _ = DRAW_INDEXED.set(draw_indexed_detour_handle);
        let _ = ThreadSuspender::for_block(|| {
            RS_SET_VIEWPORTS.get().expect("just set").enable().ok();
            RS_SET_SCISSOR_RECTS.get().expect("just set").enable().ok();
            DRAW_INDEXED.get().expect("just set").enable().ok();
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        });
        tracing::info!("single-pass: viewport + draw-doubling COM detours installed");
    }
}

static CB13: Mutex<Cb13Buffer> = Mutex::new(Cb13Buffer { buffer: None });
static IN_GBUFFER_RANGE: AtomicBool = AtomicBool::new(false);

static CAPABILITY: AtomicU8 = AtomicU8::new(Capability::Unprobed as u8);
static PATCHED: AtomicUsize = AtomicUsize::new(0);
static NO_REFS: AtomicUsize = AtomicUsize::new(0);
static DEFERRED: AtomicUsize = AtomicUsize::new(0);
static ERRORED: AtomicUsize = AtomicUsize::new(0);
