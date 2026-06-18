//! Detours on the engine's `Graphics::` GPU-call wrappers, for the render trace.
//!
//! `Draw`/`DrawIndexed` are the universal draw chokepoint (every render block routes its geometry
//! through one of them -- see `CRenderBlockGeneralJC3::Draw` at 0x1401426E0), so we *count* them per
//! eye (`DRAW_CALLS` / `DRAW_INDEXED_CALLS`) rather than trace each. The buffer-flow wrappers
//! (`SetRenderSetup`/`Clear`/`CopySurfaceToTexture`/`ResolveSurface`) fire far less often, so we
//! trace each -- letting us see which targets a dispatch binds, clears, copies and resolves.

use std::ffi::c_void;
use std::sync::atomic::Ordering;

use detours_macro::detour;
use re_utilities::hook_library::HookLibrary;

use crate::TraceEvent;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&DRAW_INDEXED_BINDER)
        .with_static_binder(&DRAW_BINDER)
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
    crate::DRAW_INDEXED_CALLS.fetch_add(1, Ordering::Relaxed);
    DRAW_INDEXED
        .get()
        .unwrap()
        .call(ctx, prim, arg2, arg3, vbuf, ibuf);
}

#[detour(address = jc3gi::graphics_engine::draw::Draw_ADDRESS)]
fn draw(ctx: *mut c_void, prim: i32, arg2: i32, arg3: i32) {
    crate::DRAW_CALLS.fetch_add(1, Ordering::Relaxed);
    DRAW.get().unwrap().call(ctx, prim, arg2, arg3);
}

#[detour(address = jc3gi::graphics_engine::draw::SetRenderSetup_ADDRESS)]
fn set_render_setup(ctx: *mut c_void, setup: *mut c_void, restore: bool) {
    crate::trace_eye(TraceEvent::SetRenderSetup {
        setup: setup as u64,
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
    crate::trace_eye(TraceEvent::Clear { color: color_rgba });
    CLEAR.get().unwrap().call(ctx, flags, color, depth, stencil);
}

#[detour(address = jc3gi::graphics_engine::draw::CopySurfaceToTexture_ADDRESS)]
fn copy_surface_to_texture(ctx: *mut c_void, dst: *mut c_void, src: *mut c_void) {
    crate::trace_eye(TraceEvent::CopySurfaceToTexture {
        dst: dst as u64,
        src: src as u64,
    });
    COPY_SURFACE_TO_TEXTURE.get().unwrap().call(ctx, dst, src);
}

#[detour(address = jc3gi::graphics_engine::draw::ResolveSurface_ADDRESS)]
fn resolve_surface(ctx: *mut c_void, params: *mut c_void) {
    crate::trace_eye(TraceEvent::ResolveSurface);
    RESOLVE_SURFACE.get().unwrap().call(ctx, params);
}
