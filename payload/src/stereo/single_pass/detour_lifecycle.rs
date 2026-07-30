//! Detour lifecycle: the `DetourSlot` wrapper, the COM-vtable install/uninstall lock, and the
//! `ensure_viewport_detours` installer that patches the D3D11 immediate-context and device vtables.
//!
//! The per-detour statics and detour functions live in their owning leaf modules (`viewport.rs`,
//! `draw_detours.rs`, `shader_detours.rs`, `per_eye_reissue.rs`); this module owns the install
//! orchestration that publishes and enables them as a batch.

use std::ffi::c_void;

use jc3gi::graphics_engine::graphics_engine::GraphicsEngine;
use parking_lot::Mutex;
use retour::{Function, GenericDetour};
use std::sync::atomic::{AtomicPtr, Ordering};
use windows::{Win32::Foundation::RECT, core::Interface};

use crate::stereo::single_pass::{
    active, draw_detours::*, per_eye_reissue::*, shader_detours::*, viewport::*,
};

/// A slot for a detour that is installed once and removed on eject.
///
/// The detour is stored as an `AtomicPtr` so the hot-path read (a `get()` per draw) is a single
/// atomic load, while the install/remove path can swap the pointer under the
/// [`DETOUR_INSTALL`](static@DETOUR_INSTALL) lock without touching the detour object itself. The
/// detour is never moved after install: `GenericDetour` owns the trampoline, and the slot owns the
/// `GenericDetour` via a `Box` whose address never changes.
///
/// -- Rust statics are not dropped, and a detour's trampoline lives in a `VirtualAlloc` region that
/// outlives the unmapped payload, so every inject/eject cycle strands one page per detour. An
/// `AtomicPtr` keeps the read on the hot path down to a single load while still allowing
/// [`take`](Self::take) to hand ownership back on eject.
pub(super) struct DetourSlot<T: Function>(AtomicPtr<GenericDetour<T>>);

impl<T: Function> DetourSlot<T> {
    pub(super) const fn new() -> Self {
        Self(AtomicPtr::new(std::ptr::null_mut()))
    }

    /// The installed detour, or `None` before install and after teardown.
    pub(super) fn get(&self) -> Option<&GenericDetour<T>> {
        // SAFETY: the pointer is null or a `Box` this slot owns. It is published with `Release`
        // before the detour it belongs to can be entered, and reclaimed only with every other thread
        // suspended, so a borrow taken here cannot outlive the allocation.
        unsafe { self.0.load(Ordering::Acquire).as_ref() }
    }

    /// Install `detour` into an empty slot. A second call leaves the slot alone and drops `detour`.
    pub(super) fn set(&self, detour: GenericDetour<T>) {
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
    pub(super) fn take(&self) -> Option<Box<GenericDetour<T>>> {
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
        let _ = re_utilities::ThreadSuspender::for_block(|| {
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
pub(super) static DETOUR_INSTALL: Mutex<()> = Mutex::new(());
