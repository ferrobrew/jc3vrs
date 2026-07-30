//! Detours on the engine's `Graphics::` GPU-call wrappers, for the render trace.
//!
//! The draw entry points (`Draw`/`DrawIndexed` and the instanced/indirect variants) and compute
//! `Dispatch`/`DispatchIndirect` fire too often to trace individually, so we *count* them: a global
//! per-eye total ([`DRAW_COUNTS`], a `DrawCounts` reported in `draw_end`) plus
//! a thread-local per-pass tally that rides along on each `SetRenderSetup` event. The instanced /
//! indirect / dispatch wrappers have unreliable demangled prototypes, so their detours forward a
//! generous set of opaque pointer-sized args transparently (preserving the full 64-bit of every
//! register/stack slot) rather than decode them -- they're only counted, never inspected. The
//! buffer-flow wrappers (`Clear`/`CopySurfaceToTexture`/`ResolveSurface`) fire rarely enough to trace.

use std::{cell::Cell, ffi::c_void, sync::atomic::Ordering};

use detours_macro::detour;
use re_utilities::hook_library::HookLibrary;

use crate::{
    debug::trace::{DrawCounts, TraceEvent, TraceState},
    hooks::graphics_engine::{clustered_lighting, reconstruction},
};

// Per-pass tallies: bumped alongside the global per-eye counters, then read + reset on each
// SetRenderSetup, so the count attached to a bind is "draws issued since the previous bind on this
// thread". Thread-local because the engine may record draws on multiple worker threads.
thread_local! {
    static PASS_DRAW: Cell<usize> = const { Cell::new(0) };
    static PASS_INDEXED: Cell<usize> = const { Cell::new(0) };
    static PASS_DISPATCH: Cell<usize> = const { Cell::new(0) };
}

/// The live per-eye draw-call counters: worker threads bump these per draw; the Draw driver clears
/// them at `draw_begin` and snapshots them into the `draw_end` event.
pub(crate) static DRAW_COUNTS: DrawCounts = DrawCounts::new();

fn bump_draw() {
    DRAW_COUNTS.draw.fetch_add(1, Ordering::Relaxed);
    PASS_DRAW.with(|c| c.set(c.get() + 1));
}

fn bump_indexed() {
    DRAW_COUNTS.draw_indexed.fetch_add(1, Ordering::Relaxed);
    PASS_INDEXED.with(|c| c.set(c.get() + 1));
}

fn bump_dispatch() {
    DRAW_COUNTS.dispatch.fetch_add(1, Ordering::Relaxed);
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
fn dispatch(ctx: *mut c_void, x: u32, y: u32, z: u32) {
    if dispatch_suppressed() {
        return;
    }
    bump_dispatch();
    DISPATCH.get().unwrap().call(ctx, x, y, z);
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
    if dispatch_suppressed() {
        return;
    }
    bump_dispatch();
    DISPATCH_INDIRECT
        .get()
        .unwrap()
        .call(a1, a2, a3, a4, a5, a6);
}

/// Whether a per-eye fullscreen-reconstruction run is in flight on this thread that must not issue the
/// compute work its block is asking for.
///
/// A scissor rectangle clips rasterization and nothing else, so a block split per eye would otherwise
/// redo its whole-texture compute on the second run, over the first run's output. The split names one
/// run to issue it on instead; this is where the other run's dispatches are dropped. A suppressed
/// dispatch is not counted either -- the tally is of work submitted, and none was. Always `false`
/// outside such a run, which is every dispatch in the engine that has nothing to do with a split.
fn dispatch_suppressed() -> bool {
    reconstruction::dispatch_suppressed()
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
    // Single-pass stereo: the bind just (re)set the viewport for this target -- including per-cascade
    // in the shadow passes -- so mirror it into viewport slot 1 for the patched shaders' viewport
    // routing. A no-op unless single-pass is active.
    crate::stereo::single_pass::duplicate_current_viewport();
    // Per-eye clustered lighting: the light-assignment target's bind is the seam the froxel split
    // narrows the viewport at, and the later binds are what put it back. A no-op unless a per-eye
    // froxel run is in flight on this thread.
    clustered_lighting::on_render_setup_bound();
    // Per-eye fullscreen reconstruction: a scissor mask is in the bound target's pixels, so an at-entry
    // per-eye run has to re-derive its eye half from the target this bind just made current. A no-op
    // unless such a run is in flight on this thread.
    reconstruction::on_render_setup_bound();
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
    // The clustered light-assignment phase's whole-target clear would wipe the first eye's half of the
    // froxel grid on the second eye's run, which is the one thing that stops the two halves composing.
    // A no-op unless that second run is in flight on this thread.
    if clustered_lighting::suppress_clear() {
        return;
    }
    CLEAR.get().unwrap().call(ctx, flags, color, depth, stencil);
}

/// Note for anyone comparing against a trace captured before the defs were corrected: this address
/// used to resolve to `Graphics::EndDraw`, which the IDB had mislabelled. Traces recorded then show
/// one `CopySurfaceToTexture` per dispatch, at its end -- that was the command-list submission, not a
/// copy. It now records the real thing: the VFX depth copy, several times per frame, mid-frame. The
/// end-of-dispatch marker those older traces happened to carry is simply gone.
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
