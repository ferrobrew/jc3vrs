#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// Parameters to [`CreateDomainProgram`](crate::graphics_engine::draw::CreateDomainProgram): the compiled DXBC bytecode, its byte length, and a debug
/// name. Same layout as [`CreateVertexProgramParams`](crate::graphics_engine::draw::CreateVertexProgramParams).
pub struct CreateDomainProgramParams {
    pub m_Code: *const u8,
    pub m_Size: u64,
    pub m_Name: *const u8,
}
fn _CreateDomainProgramParams_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], CreateDomainProgramParams>([0u8; 0x18]);
    }
    unreachable!()
}
impl CreateDomainProgramParams {}
impl std::convert::AsRef<CreateDomainProgramParams> for CreateDomainProgramParams {
    fn as_ref(&self) -> &CreateDomainProgramParams {
        self
    }
}
impl std::convert::AsMut<CreateDomainProgramParams> for CreateDomainProgramParams {
    fn as_mut(&mut self) -> &mut CreateDomainProgramParams {
        self
    }
}
#[repr(C, align(8))]
/// Parameters to [`CreateFragmentProgram`](crate::graphics_engine::draw::CreateFragmentProgram): the compiled DXBC bytecode (`m_Code`) and its byte length
/// (`m_Size`), passed straight through to `ID3D11Device::CreatePixelShader`. `m_Size` is read as a
/// pointer-width value (the bytecode length argument to `CreatePixelShader`).
pub struct CreateFragmentProgramParams {
    pub m_Code: *const u8,
    pub m_Size: u64,
}
fn _CreateFragmentProgramParams_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], CreateFragmentProgramParams>([0u8; 0x10]);
    }
    unreachable!()
}
impl CreateFragmentProgramParams {}
impl std::convert::AsRef<CreateFragmentProgramParams> for CreateFragmentProgramParams {
    fn as_ref(&self) -> &CreateFragmentProgramParams {
        self
    }
}
impl std::convert::AsMut<CreateFragmentProgramParams> for CreateFragmentProgramParams {
    fn as_mut(&mut self) -> &mut CreateFragmentProgramParams {
        self
    }
}
#[repr(C, align(8))]
/// Parameters to [`CreateHullProgram`](crate::graphics_engine::draw::CreateHullProgram): the compiled DXBC bytecode, its byte length, and a debug name.
/// Same layout as [`CreateVertexProgramParams`](crate::graphics_engine::draw::CreateVertexProgramParams).
pub struct CreateHullProgramParams {
    pub m_Code: *const u8,
    pub m_Size: u64,
    pub m_Name: *const u8,
}
fn _CreateHullProgramParams_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], CreateHullProgramParams>([0u8; 0x18]);
    }
    unreachable!()
}
impl CreateHullProgramParams {}
impl std::convert::AsRef<CreateHullProgramParams> for CreateHullProgramParams {
    fn as_ref(&self) -> &CreateHullProgramParams {
        self
    }
}
impl std::convert::AsMut<CreateHullProgramParams> for CreateHullProgramParams {
    fn as_mut(&mut self) -> &mut CreateHullProgramParams {
        self
    }
}
#[repr(C, align(8))]
/// Parameters to [`CreateVertexProgram`](crate::graphics_engine::draw::CreateVertexProgram): the compiled DXBC bytecode, its byte length, and a debug
/// name (attached to the D3D object and used to size the retained bytecode copy).
pub struct CreateVertexProgramParams {
    pub m_Code: *const u8,
    pub m_Size: u64,
    pub m_Name: *const u8,
}
fn _CreateVertexProgramParams_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], CreateVertexProgramParams>([0u8; 0x18]);
    }
    unreachable!()
}
impl CreateVertexProgramParams {}
impl std::convert::AsRef<CreateVertexProgramParams> for CreateVertexProgramParams {
    fn as_ref(&self) -> &CreateVertexProgramParams {
        self
    }
}
impl std::convert::AsMut<CreateVertexProgramParams> for CreateVertexProgramParams {
    fn as_mut(&mut self) -> &mut CreateVertexProgramParams {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The primitive topology passed to the draw wrappers. The patchlist variants are tessellation
/// control-point counts.
pub enum PrimitiveType {
    PRIMTYPE_POINTLIST = 1isize as _,
    PRIMTYPE_LINES = 2isize as _,
    PRIMTYPE_LINE_STRIP = 3isize as _,
    PRIMTYPE_TRIANGLES = 4isize as _,
    PRIMTYPE_TRIANGLE_STRIP = 5isize as _,
    PRIMTYPE_LINE_LOOP = 6isize as _,
    PRIMTYPE_TRIANGLE_FAN = 7isize as _,
    PRIMTYPE_PATCHLIST_1 = 33isize as _,
    PRIMTYPE_PATCHLIST_2 = 34isize as _,
    PRIMTYPE_PATCHLIST_3 = 35isize as _,
    PRIMTYPE_PATCHLIST_4 = 36isize as _,
}
fn _PrimitiveType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], PrimitiveType>([0u8; 0x4]);
    }
    unreachable!()
}
pub const DrawIndexed_ADDRESS: usize = 0x141967720;
/// An indexed draw.
pub unsafe fn DrawIndexed(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    prim: crate::graphics_engine::draw::PrimitiveType,
    arg2: i32,
    arg3: i32,
    vbuf: *mut ::std::ffi::c_void,
    ibuf: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            prim: crate::graphics_engine::draw::PrimitiveType,
            arg2: i32,
            arg3: i32,
            vbuf: *mut ::std::ffi::c_void,
            ibuf: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(DrawIndexed_ADDRESS);
        f(ctx, prim, arg2, arg3, vbuf, ibuf)
    }
}
pub const Draw_ADDRESS: usize = 0x141967680;
/// A non-indexed draw.
pub unsafe fn Draw(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    prim: crate::graphics_engine::draw::PrimitiveType,
    arg2: i32,
    arg3: i32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            prim: crate::graphics_engine::draw::PrimitiveType,
            arg2: i32,
            arg3: i32,
        ) = ::std::mem::transmute(Draw_ADDRESS);
        f(ctx, prim, arg2, arg3)
    }
}
pub const DrawInstanced_ADDRESS: usize = 0x141962F10;
/// An instanced draw.
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
/// An indexed instanced draw.
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
/// A GPU-driven instanced draw.
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
/// A GPU-driven indexed instanced draw.
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
/// A compute dispatch.
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
/// A GPU-driven compute dispatch.
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
/// Binds a render setup, the render-target configuration a pass draws into.
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
/// Clears the currently-bound render setup. `color` is a 4-float RGBA pointer and may be null.
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
pub const CopySurfaceToTexture_ADDRESS: usize = 0x141954850;
/// Copies one surface into another texture: a whole-resource `ID3D11DeviceContext::CopyResource`
/// of `src`'s D3D11 resource into `dst`'s, issued under the context mutex. The release build's
/// symbol table misplaces this function's name onto [`EndDraw`](crate::graphics_engine::draw::EndDraw) and labels this address
/// `NGraphicsEngine::CGPUProfiler::BeginScope`; the body is unambiguous.
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
pub const EndDraw_ADDRESS: usize = 0x14195ABA0;
/// Submits the frame's recorded command work: under the context's critical section it indexes the
/// context's command-slot ring and submits the two prebuilt command objects for the current slot
/// through the backend. It takes no arguments beyond the context.
///
/// The release build's symbol table labels this address `Graphics::CopySurfaceToTexture`; the two
/// names are swapped (see [`CopySurfaceToTexture`](crate::graphics_engine::draw::CopySurfaceToTexture)).
pub unsafe fn EndDraw(ctx: *mut crate::graphics_engine::graphics_engine::HContext_t) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        ) = ::std::mem::transmute(EndDraw_ADDRESS);
        f(ctx)
    }
}
pub const ResolveSurface_ADDRESS: usize = 0x1419672B0;
/// Resolves an MSAA surface.
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
pub const GetRTVFromSurface_ADDRESS: usize = 0x141956240;
/// Returns the surface's render-target view.
pub unsafe fn GetRTVFromSurface(
    surface: *mut ::std::ffi::c_void,
) -> *mut ::std::ffi::c_void {
    unsafe {
        let f: unsafe extern "system" fn(
            surface: *mut ::std::ffi::c_void,
        ) -> *mut ::std::ffi::c_void = ::std::mem::transmute(GetRTVFromSurface_ADDRESS);
        f(surface)
    }
}
pub const GetDSVFromSurface_ADDRESS: usize = 0x141956250;
/// Returns the surface's depth-stencil view.
pub unsafe fn GetDSVFromSurface(
    surface: *mut ::std::ffi::c_void,
) -> *mut ::std::ffi::c_void {
    unsafe {
        let f: unsafe extern "system" fn(
            surface: *mut ::std::ffi::c_void,
        ) -> *mut ::std::ffi::c_void = ::std::mem::transmute(GetDSVFromSurface_ADDRESS);
        f(surface)
    }
}
pub const CreateFragmentProgram_ADDRESS: usize = 0x141953470;
/// The leaf fragment-program creator: it wraps `ID3D11Device::CreatePixelShader` over
/// `params.m_Code`/`params.m_Size`. `CreatePixelShader` copies the bytecode, so `params.m_Code` need
/// not remain valid past the call, and the returned holder retains no bytecode of its own. Static (no
/// `this`); the first argument is the graphics device.
pub unsafe fn CreateFragmentProgram(
    device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
    params: *mut crate::graphics_engine::draw::CreateFragmentProgramParams,
) -> *mut ::std::ffi::c_void {
    unsafe {
        let f: unsafe extern "system" fn(
            device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
            params: *mut crate::graphics_engine::draw::CreateFragmentProgramParams,
        ) -> *mut ::std::ffi::c_void = ::std::mem::transmute(
            CreateFragmentProgram_ADDRESS,
        );
        f(device, params)
    }
}
pub const CreateVertexProgram_ADDRESS: usize = 0x141953320;
/// The leaf vertex-program creator: it wraps `ID3D11Device::CreateVertexShader` over
/// `params.m_Code`/`params.m_Size`. Unlike [`CreateFragmentProgram`](crate::graphics_engine::draw::CreateFragmentProgram), the returned holder also
/// retains a heap copy of the bytecode (used later for input-layout creation), so both the D3D
/// object and the retained copy reflect whatever bytecode was passed in. Static (no `this`); the
/// first argument is the graphics device.
pub unsafe fn CreateVertexProgram(
    device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
    params: *const crate::graphics_engine::draw::CreateVertexProgramParams,
) -> *mut ::std::ffi::c_void {
    unsafe {
        let f: unsafe extern "system" fn(
            device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
            params: *const crate::graphics_engine::draw::CreateVertexProgramParams,
        ) -> *mut ::std::ffi::c_void = ::std::mem::transmute(
            CreateVertexProgram_ADDRESS,
        );
        f(device, params)
    }
}
pub const CreateHullProgram_ADDRESS: usize = 0x141953690;
/// The leaf hull-program creator: it wraps `ID3D11Device::CreateHullShader` (vtable slot 16) over
/// `params.m_Code`/`params.m_Size`. `CreateHullShader` copies the bytecode, so `params.m_Code` need not
/// remain valid past the call; unlike [`CreateVertexProgram`](crate::graphics_engine::draw::CreateVertexProgram), the returned holder is a bare shader
/// pointer with no retained bytecode copy. Static (no `this`); the first argument is the graphics
/// device.
pub unsafe fn CreateHullProgram(
    device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
    params: *const crate::graphics_engine::draw::CreateHullProgramParams,
) -> *mut ::std::ffi::c_void {
    unsafe {
        let f: unsafe extern "system" fn(
            device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
            params: *const crate::graphics_engine::draw::CreateHullProgramParams,
        ) -> *mut ::std::ffi::c_void = ::std::mem::transmute(CreateHullProgram_ADDRESS);
        f(device, params)
    }
}
pub const CreateDomainProgram_ADDRESS: usize = 0x1419537A0;
/// The leaf domain-program creator: it wraps `ID3D11Device::CreateDomainShader` (vtable slot 17) over
/// `params.m_Code`/`params.m_Size`. `CreateDomainShader` copies the bytecode, so `params.m_Code` need
/// not remain valid past the call; like [`CreateHullProgram`](crate::graphics_engine::draw::CreateHullProgram), the returned holder is a bare shader
/// pointer with no retained bytecode copy. Static (no `this`); the first argument is the graphics
/// device.
pub unsafe fn CreateDomainProgram(
    device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
    params: *const crate::graphics_engine::draw::CreateDomainProgramParams,
) -> *mut ::std::ffi::c_void {
    unsafe {
        let f: unsafe extern "system" fn(
            device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
            params: *const crate::graphics_engine::draw::CreateDomainProgramParams,
        ) -> *mut ::std::ffi::c_void = ::std::mem::transmute(
            CreateDomainProgram_ADDRESS,
        );
        f(device, params)
    }
}
pub const SetFragmentProgramConstants_ADDRESS: usize = 0x141964840;
/// Stages `count` float4 constants into the given fragment constant buffer slot, starting at
/// `start_offset`, writing to the context's per-slot staging buffer and setting the dirty flag. The
/// actual GPU upload happens at the next state flush.
pub unsafe fn SetFragmentProgramConstants(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    cb_index: i32,
    start_offset: u32,
    data: *const f32,
    count: u32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            cb_index: i32,
            start_offset: u32,
            data: *const f32,
            count: u32,
        ) = ::std::mem::transmute(SetFragmentProgramConstants_ADDRESS);
        f(ctx, cb_index, start_offset, data, count)
    }
}
pub const SetVertexProgramConstants_ADDRESS: usize = 0x141964740;
/// Stages `count` float4 constants into the given **vertex** constant buffer slot, starting at
/// `start_offset` (in float4 rows), writing to the context's per-slot staging buffer and setting the
/// dirty flag; the GPU upload happens at the next state flush. The vertex analogue of
/// [`SetFragmentProgramConstants`](crate::graphics_engine::draw::SetFragmentProgramConstants). `cb_index` is the vertex-shader constant-buffer slot (e.g. `1`
/// for `cb1`). A row-level redundancy check skips rows byte-identical to what is already staged.
pub unsafe fn SetVertexProgramConstants(
    ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    cb_index: i32,
    start_offset: u32,
    data: *const f32,
    count: u32,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            cb_index: i32,
            start_offset: u32,
            data: *const f32,
            count: u32,
        ) = ::std::mem::transmute(SetVertexProgramConstants_ADDRESS);
        f(ctx, cb_index, start_offset, data, count)
    }
}
