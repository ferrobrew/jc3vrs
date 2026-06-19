#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
pub const DrawIndexed_ADDRESS: usize = 0x141967720;
/// Indexed draw (`Graphics::DrawIndexed`).
pub unsafe fn DrawIndexed(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    prim: i32,
    arg2: i32,
    arg3: i32,
    vbuf: *mut ::std::ffi::c_void,
    ibuf: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            prim: i32,
            arg2: i32,
            arg3: i32,
            vbuf: *mut ::std::ffi::c_void,
            ibuf: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(DrawIndexed_ADDRESS);
        f(ctx, prim, arg2, arg3, vbuf, ibuf)
    }
}
pub const Draw_ADDRESS: usize = 0x141967680;
/// Non-indexed draw (`Graphics::Draw`).
pub unsafe fn Draw(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    prim: i32,
    arg2: i32,
    arg3: i32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            prim: i32,
            arg2: i32,
            arg3: i32,
        ) = ::std::mem::transmute(Draw_ADDRESS);
        f(ctx, prim, arg2, arg3)
    }
}
pub const DrawInstanced_ADDRESS: usize = 0x141962F10;
/// Instanced draw (`Graphics::DrawInstanced`).
pub unsafe fn DrawInstanced(
    a1: *mut ::std::ffi::c_void,
    a2: *mut ::std::ffi::c_void,
    a3: *mut ::std::ffi::c_void,
    a4: *mut ::std::ffi::c_void,
    a5: *mut ::std::ffi::c_void,
    a6: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            a1: *mut ::std::ffi::c_void,
            a2: *mut ::std::ffi::c_void,
            a3: *mut ::std::ffi::c_void,
            a4: *mut ::std::ffi::c_void,
            a5: *mut ::std::ffi::c_void,
            a6: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(DrawInstanced_ADDRESS);
        f(a1, a2, a3, a4, a5, a6)
    }
}
pub const DrawIndexedInstanced_ADDRESS: usize = 0x141962E80;
/// Indexed instanced draw (`Graphics::DrawIndexedInstanced`).
pub unsafe fn DrawIndexedInstanced(
    a1: *mut ::std::ffi::c_void,
    a2: *mut ::std::ffi::c_void,
    a3: *mut ::std::ffi::c_void,
    a4: *mut ::std::ffi::c_void,
    a5: *mut ::std::ffi::c_void,
    a6: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            a1: *mut ::std::ffi::c_void,
            a2: *mut ::std::ffi::c_void,
            a3: *mut ::std::ffi::c_void,
            a4: *mut ::std::ffi::c_void,
            a5: *mut ::std::ffi::c_void,
            a6: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(DrawIndexedInstanced_ADDRESS);
        f(a1, a2, a3, a4, a5, a6)
    }
}
pub const DrawInstancedIndirect_ADDRESS: usize = 0x141962CC0;
/// GPU-driven instanced draw (`Graphics::DrawInstancedIndirect`).
pub unsafe fn DrawInstancedIndirect(
    a1: *mut ::std::ffi::c_void,
    a2: *mut ::std::ffi::c_void,
    a3: *mut ::std::ffi::c_void,
    a4: *mut ::std::ffi::c_void,
    a5: *mut ::std::ffi::c_void,
    a6: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            a1: *mut ::std::ffi::c_void,
            a2: *mut ::std::ffi::c_void,
            a3: *mut ::std::ffi::c_void,
            a4: *mut ::std::ffi::c_void,
            a5: *mut ::std::ffi::c_void,
            a6: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(DrawInstancedIndirect_ADDRESS);
        f(a1, a2, a3, a4, a5, a6)
    }
}
pub const DrawIndexedInstancedIndirect_ADDRESS: usize = 0x141963080;
/// GPU-driven indexed instanced draw (`Graphics::DrawIndexedInstancedIndirect`).
pub unsafe fn DrawIndexedInstancedIndirect(
    a1: *mut ::std::ffi::c_void,
    a2: *mut ::std::ffi::c_void,
    a3: *mut ::std::ffi::c_void,
    a4: *mut ::std::ffi::c_void,
    a5: *mut ::std::ffi::c_void,
    a6: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            a1: *mut ::std::ffi::c_void,
            a2: *mut ::std::ffi::c_void,
            a3: *mut ::std::ffi::c_void,
            a4: *mut ::std::ffi::c_void,
            a5: *mut ::std::ffi::c_void,
            a6: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(DrawIndexedInstancedIndirect_ADDRESS);
        f(a1, a2, a3, a4, a5, a6)
    }
}
pub const Dispatch_ADDRESS: usize = 0x141962AD0;
/// Compute dispatch (`Graphics::Dispatch`).
pub unsafe fn Dispatch(
    a1: *mut ::std::ffi::c_void,
    a2: *mut ::std::ffi::c_void,
    a3: *mut ::std::ffi::c_void,
    a4: *mut ::std::ffi::c_void,
    a5: *mut ::std::ffi::c_void,
    a6: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            a1: *mut ::std::ffi::c_void,
            a2: *mut ::std::ffi::c_void,
            a3: *mut ::std::ffi::c_void,
            a4: *mut ::std::ffi::c_void,
            a5: *mut ::std::ffi::c_void,
            a6: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(Dispatch_ADDRESS);
        f(a1, a2, a3, a4, a5, a6)
    }
}
pub const DispatchIndirect_ADDRESS: usize = 0x141962B60;
/// GPU-driven compute dispatch (`Graphics::DispatchIndirect`).
pub unsafe fn DispatchIndirect(
    a1: *mut ::std::ffi::c_void,
    a2: *mut ::std::ffi::c_void,
    a3: *mut ::std::ffi::c_void,
    a4: *mut ::std::ffi::c_void,
    a5: *mut ::std::ffi::c_void,
    a6: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            a1: *mut ::std::ffi::c_void,
            a2: *mut ::std::ffi::c_void,
            a3: *mut ::std::ffi::c_void,
            a4: *mut ::std::ffi::c_void,
            a5: *mut ::std::ffi::c_void,
            a6: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(DispatchIndirect_ADDRESS);
        f(a1, a2, a3, a4, a5, a6)
    }
}
pub const SetRenderSetup_ADDRESS: usize = 0x141966D20;
/// Bind a render setup -- the render-target configuration a pass draws into.
pub unsafe fn SetRenderSetup(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    setup: *mut ::std::ffi::c_void,
    restore: bool,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            setup: *mut ::std::ffi::c_void,
            restore: bool,
        ) = ::std::mem::transmute(SetRenderSetup_ADDRESS);
        f(ctx, setup, restore)
    }
}
pub const Clear_ADDRESS: usize = 0x141967020;
/// Clear the currently-bound render setup (`color` is a 4-float RGBA pointer, may be null).
pub unsafe fn Clear(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    flags: u32,
    color: *mut ::std::ffi::c_void,
    depth: f32,
    stencil: u32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            flags: u32,
            color: *mut ::std::ffi::c_void,
            depth: f32,
            stencil: u32,
        ) = ::std::mem::transmute(Clear_ADDRESS);
        f(ctx, flags, color, depth, stencil)
    }
}
pub const CopySurfaceToTexture_ADDRESS: usize = 0x14195ABA0;
/// Copy one surface into another texture.
pub unsafe fn CopySurfaceToTexture(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    dst: *mut ::std::ffi::c_void,
    src: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            dst: *mut ::std::ffi::c_void,
            src: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(CopySurfaceToTexture_ADDRESS);
        f(ctx, dst, src)
    }
}
pub const ResolveSurface_ADDRESS: usize = 0x1419672B0;
/// Resolve an MSAA surface.
pub unsafe fn ResolveSurface(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    params: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            params: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(ResolveSurface_ADDRESS);
        f(ctx, params)
    }
}
