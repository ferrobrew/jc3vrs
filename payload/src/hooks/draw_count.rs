//! Detours on the engine's `Graphics::` GPU-call wrappers, for the render trace.
//!
//! The draw entry points (`Draw`/`DrawIndexed` and the instanced/indirect variants) and compute
//! `Dispatch`/`DispatchIndirect` fire too often to trace individually, so we *count* them: a global
//! per-eye total (`DRAW_CALLS` / `DRAW_INDEXED_CALLS` / `DISPATCH_CALLS`, reported in `draw_end`) plus
//! a thread-local per-pass tally that rides along on each `SetRenderSetup` event. The instanced /
//! indirect / dispatch wrappers have unreliable demangled prototypes, so their detours forward a
//! generous set of opaque pointer-sized args transparently (preserving the full 64-bit of every
//! register/stack slot) rather than decode them -- they're only counted, never inspected. The
//! buffer-flow wrappers (`Clear`/`CopySurfaceToTexture`/`ResolveSurface`) fire rarely enough to trace.

use std::{cell::Cell, ffi::c_void, sync::atomic::Ordering};

use detours_macro::detour;
use re_utilities::hook_library::HookLibrary;

use crate::trace::{TraceEvent, TraceState};

// Per-pass tallies: bumped alongside the global per-eye counters, then read + reset on each
// SetRenderSetup, so the count attached to a bind is "draws issued since the previous bind on this
// thread". Thread-local because the engine may record draws on multiple worker threads.
thread_local! {
    static PASS_DRAW: Cell<usize> = const { Cell::new(0) };
    static PASS_INDEXED: Cell<usize> = const { Cell::new(0) };
    static PASS_DISPATCH: Cell<usize> = const { Cell::new(0) };
}

fn bump_draw() {
    crate::DRAW_CALLS.fetch_add(1, Ordering::Relaxed);
    PASS_DRAW.with(|c| c.set(c.get() + 1));
}

fn bump_indexed() {
    crate::DRAW_INDEXED_CALLS.fetch_add(1, Ordering::Relaxed);
    PASS_INDEXED.with(|c| c.set(c.get() + 1));
}

fn bump_dispatch() {
    crate::DISPATCH_CALLS.fetch_add(1, Ordering::Relaxed);
    PASS_DISPATCH.with(|c| c.set(c.get() + 1));
}

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&DRAW_INDEXED_BINDER)
        .with_static_binder(&DRAW_BINDER)
        .with_static_binder(&DRAW_INSTANCED_BINDER)
        .with_static_binder(&DRAW_INDEXED_INSTANCED_BINDER)
        .with_static_binder(&DRAW_INSTANCED_INDIRECT_BINDER)
        .with_static_binder(&DRAW_INDEXED_INSTANCED_INDIRECT_BINDER)
        .with_static_binder(&DISPATCH_BINDER)
        .with_static_binder(&DISPATCH_INDIRECT_BINDER)
        .with_static_binder(&SET_RENDER_SETUP_BINDER)
        .with_static_binder(&CLEAR_BINDER)
        .with_static_binder(&COPY_SURFACE_TO_TEXTURE_BINDER)
        .with_static_binder(&RESOLVE_SURFACE_BINDER)
}

#[detour(address = jc3gi::graphics_engine::draw::DrawIndexed_ADDRESS)]
fn draw_indexed(
    ctx: *mut c_void,
    prim: i32,
    arg2: i32,
    arg3: i32,
    vbuf: *mut c_void,
    ibuf: *mut c_void,
) {
    bump_indexed();
    DRAW_INDEXED
        .get()
        .unwrap()
        .call(ctx, prim, arg2, arg3, vbuf, ibuf);
}

#[detour(address = jc3gi::graphics_engine::draw::Draw_ADDRESS)]
fn draw(ctx: *mut c_void, prim: i32, arg2: i32, arg3: i32) {
    bump_draw();
    DRAW.get().unwrap().call(ctx, prim, arg2, arg3);
}

// The six below forward opaque args transparently -- see the module doc and draw.pyxis.

#[detour(address = jc3gi::graphics_engine::draw::DrawInstanced_ADDRESS)]
fn draw_instanced(
    a1: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
    a4: *mut c_void,
    a5: *mut c_void,
    a6: *mut c_void,
) {
    bump_draw();
    DRAW_INSTANCED.get().unwrap().call(a1, a2, a3, a4, a5, a6);
}

#[detour(address = jc3gi::graphics_engine::draw::DrawIndexedInstanced_ADDRESS)]
fn draw_indexed_instanced(
    a1: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
    a4: *mut c_void,
    a5: *mut c_void,
    a6: *mut c_void,
) {
    bump_indexed();
    DRAW_INDEXED_INSTANCED
        .get()
        .unwrap()
        .call(a1, a2, a3, a4, a5, a6);
}

#[detour(address = jc3gi::graphics_engine::draw::DrawInstancedIndirect_ADDRESS)]
fn draw_instanced_indirect(
    a1: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
    a4: *mut c_void,
    a5: *mut c_void,
    a6: *mut c_void,
) {
    bump_draw();
    DRAW_INSTANCED_INDIRECT
        .get()
        .unwrap()
        .call(a1, a2, a3, a4, a5, a6);
}

#[detour(address = jc3gi::graphics_engine::draw::DrawIndexedInstancedIndirect_ADDRESS)]
fn draw_indexed_instanced_indirect(
    a1: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
    a4: *mut c_void,
    a5: *mut c_void,
    a6: *mut c_void,
) {
    bump_indexed();
    DRAW_INDEXED_INSTANCED_INDIRECT
        .get()
        .unwrap()
        .call(a1, a2, a3, a4, a5, a6);
}

#[detour(address = jc3gi::graphics_engine::draw::Dispatch_ADDRESS)]
fn dispatch(
    a1: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
    a4: *mut c_void,
    a5: *mut c_void,
    a6: *mut c_void,
) {
    bump_dispatch();
    DISPATCH.get().unwrap().call(a1, a2, a3, a4, a5, a6);
}

#[detour(address = jc3gi::graphics_engine::draw::DispatchIndirect_ADDRESS)]
fn dispatch_indirect(
    a1: *mut c_void,
    a2: *mut c_void,
    a3: *mut c_void,
    a4: *mut c_void,
    a5: *mut c_void,
    a6: *mut c_void,
) {
    bump_dispatch();
    DISPATCH_INDIRECT
        .get()
        .unwrap()
        .call(a1, a2, a3, a4, a5, a6);
}

#[detour(address = jc3gi::graphics_engine::draw::SetRenderSetup_ADDRESS)]
fn set_render_setup(ctx: *mut c_void, setup: *mut c_void, restore: bool) {
    // Flush this thread's per-pass tally onto the bind: counts are the draws issued into the
    // previously-bound target since the last SetRenderSetup.
    TraceState::record_eye(TraceEvent::SetRenderSetup {
        setup: setup as u64,
        draws: PASS_DRAW.with(|c| c.replace(0)),
        indexed: PASS_INDEXED.with(|c| c.replace(0)),
        dispatch: PASS_DISPATCH.with(|c| c.replace(0)),
    });
    SET_RENDER_SETUP.get().unwrap().call(ctx, setup, restore);
}

#[detour(address = jc3gi::graphics_engine::draw::Clear_ADDRESS)]
fn clear(ctx: *mut c_void, flags: u32, color: *mut c_void, depth: f32, stencil: u32) {
    let color_rgba = unsafe {
        let p = color as *const f32;
        if p.is_null() {
            [0.0; 4]
        } else {
            [p.read(), p.add(1).read(), p.add(2).read(), p.add(3).read()]
        }
    };
    TraceState::record_eye(TraceEvent::Clear { color: color_rgba });
    CLEAR.get().unwrap().call(ctx, flags, color, depth, stencil);
}

#[detour(address = jc3gi::graphics_engine::draw::CopySurfaceToTexture_ADDRESS)]
fn copy_surface_to_texture(ctx: *mut c_void, dst: *mut c_void, src: *mut c_void) {
    TraceState::record_eye(TraceEvent::CopySurfaceToTexture {
        dst: dst as u64,
        src: src as u64,
    });
    COPY_SURFACE_TO_TEXTURE.get().unwrap().call(ctx, dst, src);
}

#[detour(address = jc3gi::graphics_engine::draw::ResolveSurface_ADDRESS)]
fn resolve_surface(ctx: *mut c_void, params: *mut c_void) {
    TraceState::record_eye(TraceEvent::ResolveSurface);
    RESOLVE_SURFACE.get().unwrap().call(ctx, params);
}
