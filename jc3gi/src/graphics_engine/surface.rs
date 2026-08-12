#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The parameters to [`Create2DTexture`](crate::graphics_engine::surface::Create2DTexture). Padding at `0x24` aligns [`m_Name`](crate::graphics_engine::surface::Create2DTextureParams::m_Name) to
/// eight bytes.
pub struct Create2DTextureParams {
    pub m_Width: u32,
    pub m_Height: u32,
    /// The array-slice count (`1` for a plain 2D texture).
    pub m_NumSlices: u32,
    pub m_NumMipLevels: u32,
    /// The ESRAM byte offset on the console backends; ignored on D3D11.
    pub m_ESRAMOffset: u32,
    pub m_Format: crate::graphics_engine::surface::SurfaceFormat,
    pub m_MultisampleType: crate::graphics_engine::surface::MultisampleFormat,
    pub m_UsageType: crate::graphics_engine::surface::UsageType,
    pub m_PoolType: crate::graphics_engine::surface::PoolType,
    _field_24: [u8; 4],
    /// A debug name for the texture. The resource-tracking builds register the created texture
    /// under `HashString(m_Name)` in the device's texture map; this build's creation path does
    /// not read it.
    pub m_Name: *const ::std::ffi::c_char,
    /// The initial contents, or null to leave the texture uninitialised.
    pub m_Data: *const ::std::ffi::c_void,
    pub m_DataSize: u32,
    pub m_DataLayout: crate::graphics_engine::surface::DataLayout,
    pub m_TileMode: crate::graphics_engine::surface::TileMode,
    /// A packed bitfield, from the low bit up: `m_PreTiled:1`, `m_PreAllocated:1`,
    /// `m_InvertedZcull:1`, `m_TiledMemory:1`, `m_GenerateMips:1`, `m_PackedMips:1`,
    /// `m_Castable:1`, `m_ColorSpace:1`, `m_UnorderedAccess:1`, `m_MipLevelClamp:4`,
    /// `m_RenderCompression:1`.
    ///
    /// [`Create2DTexture`](crate::graphics_engine::surface::Create2DTexture) reads four of them on D3D11: `m_ColorSpace` (bit 7) requests the sRGB
    /// variant of the format, `m_Castable` (bit 6) creates the resource in the typeless
    /// equivalent of the format so views may reinterpret it, `m_UnorderedAccess` (bit 8) adds a
    /// UAV bind and creates the unordered-access view, and `m_MipLevelClamp` (bits 9-12)
    /// becomes the view's most-detailed mip.
    pub m_Flags: u32,
    /// The allocation tag: `Group:6`, `Pool:1`, `Asset:1` from the low bit up.
    pub m_Tag: u8,
    _field_49: [u8; 7],
}
fn _Create2DTextureParams_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x50], Create2DTextureParams>([0u8; 0x50]);
    }
    unreachable!()
}
impl Create2DTextureParams {}
impl std::convert::AsRef<Create2DTextureParams> for Create2DTextureParams {
    fn as_ref(&self) -> &Create2DTextureParams {
        self
    }
}
impl std::convert::AsMut<Create2DTextureParams> for Create2DTextureParams {
    fn as_mut(&mut self) -> &mut Create2DTextureParams {
        self
    }
}
#[repr(C, align(8))]
/// The parameters to [`CreateRenderSetup`](crate::graphics_engine::surface::CreateRenderSetup): the output targets a pass draws into. No dimensions
/// appear here — a render setup's size is whatever its bound targets carry.
pub struct CreateRenderSetupParams {
    /// The depth-stencil target, or null for a colour-only setup.
    pub m_DepthTarget: *mut crate::graphics_engine::graphics_engine::HTexture_t,
    /// The colour targets, in `OMSetRenderTargets` order. The engine's own name for the first
    /// entry is `m_ColorTarget`, which shares storage with the array. The count is derived at
    /// creation by scanning for the first null, so the entries must be packed from `[0]`.
    pub m_ColorTargets: [*mut crate::graphics_engine::graphics_engine::HTexture_t; 8],
    /// The buffer unordered-access views bound alongside the targets, likewise counted by
    /// scanning for the first null.
    pub m_UAVs: [*mut ::std::ffi::c_void; 8],
    /// The texture unordered-access views, likewise counted by scanning for the first null.
    pub m_TextureUAVs: [*mut crate::graphics_engine::graphics_engine::HTexture_t; 8],
    pub m_MultisampleFormat: crate::graphics_engine::surface::MultisampleFormat,
    /// The colour-write mask stored on the setup.
    pub m_Mask: u32,
    /// A packed bitfield, from the low bit up: `m_AutoResolve:1`, `m_EDRAMLayout:2`,
    /// `m_UAVStart:4`. [`CreateRenderSetup`](crate::graphics_engine::surface::CreateRenderSetup) reads `m_UAVStart` (bits 3-6) as the register slot
    /// the unordered-access views start at; the sentinel `15` means "immediately after the
    /// colour targets", and is replaced by the derived colour-target count.
    pub m_Flags: u32,
    /// The EDRAM base on the console backends; ignored on D3D11.
    pub m_Base: u16,
    /// The EDRAM hierarchical-Z base on the console backends; ignored on D3D11.
    pub m_HiZBase: u16,
    /// An explicit EDRAM layout for the console backends; null on D3D11.
    pub m_ManualLayout: *mut ::std::ffi::c_void,
}
fn _CreateRenderSetupParams_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xE0], CreateRenderSetupParams>([0u8; 0xE0]);
    }
    unreachable!()
}
impl CreateRenderSetupParams {}
impl std::convert::AsRef<CreateRenderSetupParams> for CreateRenderSetupParams {
    fn as_ref(&self) -> &CreateRenderSetupParams {
        self
    }
}
impl std::convert::AsMut<CreateRenderSetupParams> for CreateRenderSetupParams {
    fn as_mut(&mut self) -> &mut CreateRenderSetupParams {
        self
    }
}
#[repr(C, align(8))]
/// The parameters to [`CreateSurfaceAlias`](crate::graphics_engine::surface::CreateSurfaceAlias).
pub struct CreateSurfaceAliasParams {
    /// The surface to alias. Its handle is copied wholesale and its D3D11 resource is
    /// referenced, not duplicated.
    pub m_SourceSurface: *mut crate::graphics_engine::graphics_engine::HTexture_t,
    /// The format the alias' views reinterpret the source resource as.
    pub m_Format: crate::graphics_engine::surface::SurfaceFormat,
    _field_c: [u8; 4],
    /// A debug name for the alias. As with [`Create2DTextureParams::m_Name`](crate::graphics_engine::surface::Create2DTextureParams::m_Name), this build's
    /// creation path does not read it.
    pub m_Name: *const ::std::ffi::c_char,
    /// A packed bitfield, from the low bit up: `m_ColorSpace:1`, `m_ReadOnlyDepth:1`,
    /// `m_ReadOnlyStencil:1`. `m_ColorSpace` requests the sRGB variant of
    /// [`m_Format`](crate::graphics_engine::surface::CreateSurfaceAliasParams::m_Format); the read-only bits select the corresponding
    /// depth-stencil-view read-only flags.
    pub m_Flags: u32,
    _field_1c: [u8; 4],
}
fn _CreateSurfaceAliasParams_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], CreateSurfaceAliasParams>([0u8; 0x20]);
    }
    unreachable!()
}
impl CreateSurfaceAliasParams {}
impl std::convert::AsRef<CreateSurfaceAliasParams> for CreateSurfaceAliasParams {
    fn as_ref(&self) -> &CreateSurfaceAliasParams {
        self
    }
}
impl std::convert::AsMut<CreateSurfaceAliasParams> for CreateSurfaceAliasParams {
    fn as_mut(&mut self) -> &mut CreateSurfaceAliasParams {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The memory ordering of the initial data handed to a texture creator.
pub enum DataLayout {
    Row = 0isize as _,
    Column = 1isize as _,
}
fn _DataLayout_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], DataLayout>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The multisample configuration of a surface or render setup. The low values double as the
/// D3D11 sample count (`None` is one sample); the CSAA entries are legacy names with no D3D11
/// equivalent.
pub enum MultisampleFormat {
    None = 1isize as _,
    X2 = 2isize as _,
    X4 = 4isize as _,
    X8 = 8isize as _,
    X8CSAA = 9isize as _,
    X16CSAA = 10isize as _,
    X16QCSAA = 11isize as _,
    X32CSAA = 12isize as _,
}
fn _MultisampleFormat_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], MultisampleFormat>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// Which memory pool a resource is allocated from. `SysMem` produces a staging resource;
/// `GfxMem` is ordinary GPU memory.
pub enum PoolType {
    GfxMem = 0isize as _,
    SysMem = 1isize as _,
}
fn _PoolType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], PoolType>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// A texture/surface pixel format. The values are the `DXGI_FORMAT` codes the backend passes
/// straight to D3D11 (so [`ABGR32`](crate::graphics_engine::surface::SurfaceFormat::ABGR32) is `DXGI_FORMAT_R8G8B8A8_UNORM`), with a handful
/// of platform-independent names above [`ARGB32`](crate::graphics_engine::surface::SurfaceFormat::ARGB32) for formats D3D11 has no direct
/// equivalent for. `Unknown` (0) means "take the format from the source data".
///
/// The engine's enumeration carries many aliases that resolve to the same code (`RGBA32`,
/// `BGRA32`, and `FB_A8R8G8B8` all share [`ABGR32`](crate::graphics_engine::surface::SurfaceFormat::ABGR32)'s code); only one name per
/// value is listed here.
pub enum SurfaceFormat {
    Unknown = 0isize as _,
    A32B32G32R32F = 2isize as _,
    RGBA32I = 4isize as _,
    A16B16G16R16F = 10isize as _,
    A16B16G16R16 = 11isize as _,
    A16B16G16R16S = 13isize as _,
    RGBA16I = 14isize as _,
    RG32I = 18isize as _,
    D32FS8 = 20isize as _,
    FB_A2R10G10B10 = 24isize as _,
    R11G11B10F = 26isize as _,
    ABGR32 = 28isize as _,
    RGBA8UI = 30isize as _,
    RGBA8I = 32isize as _,
    G16R16F = 34isize as _,
    G16R16 = 35isize as _,
    RG16UI = 36isize as _,
    RG16I = 38isize as _,
    D32F = 40isize as _,
    R32F = 41isize as _,
    R32I = 43isize as _,
    D24S8 = 45isize as _,
    G8R8 = 49isize as _,
    RG8UI = 50isize as _,
    V8U8 = 51isize as _,
    RG8I = 52isize as _,
    R16F = 54isize as _,
    D16 = 55isize as _,
    R16 = 56isize as _,
    R16UI = 57isize as _,
    R16I = 59isize as _,
    R8 = 61isize as _,
    R8UI = 62isize as _,
    R8I = 64isize as _,
    DXT1 = 71isize as _,
    DXT3 = 74isize as _,
    DXT5 = 77isize as _,
    BC4 = 80isize as _,
    BC5 = 83isize as _,
    BC6_UF16 = 95isize as _,
    BC6_SF16 = 96isize as _,
    BC7 = 98isize as _,
    ARGB32 = 1000isize as _,
    ARGB4444 = 1001isize as _,
    RGB565 = 1002isize as _,
    A8L8 = 1003isize as _,
}
fn _SurfaceFormat_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], SurfaceFormat>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The tiling/swizzle mode of a texture's memory. Only `Linear` is meaningful on D3D11; the
/// other entries exist for the console backends.
pub enum TileMode {
    Linear = -1isize as _,
    MaxValue = 31isize as _,
    Lookup = 254isize as _,
    None = 255isize as _,
}
fn _TileMode_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], TileMode>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// How a resource is written to, which selects the D3D11 usage and bind flags at creation.
/// [`Create2DTexture`](crate::graphics_engine::surface::Create2DTexture) maps `RenderTarget` to a render-target (or depth-stencil, for a depth
/// format) bind, `Dynamic` to a dynamic/CPU-writable resource, and `Default`/`Immutable` to a
/// plain default-usage resource.
pub enum UsageType {
    Default = 0isize as _,
    Immutable = 1isize as _,
    Dynamic = 2isize as _,
    RenderTarget = 4isize as _,
}
fn _UsageType_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], UsageType>([0u8; 0x4]);
    }
    unreachable!()
}
pub const Create2DTexture_ADDRESS: usize = 0x141958960;
/// Creates a 2D texture: allocates the `0x58`-byte texture handle, creates the D3D11 resource
/// from `params`, and creates the views the usage implies (a shader-resource view for
/// non-staging textures, plus an unordered-access view when the params ask for one). A creation
/// failure routes through the graphics critical-error path rather than returning null.
///
/// A texture is not by itself drawable: a render target is obtained from it with
/// [`GetRenderTarget`](crate::graphics_engine::surface::GetRenderTarget).
pub unsafe fn Create2DTexture(
    device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
    params: *const crate::graphics_engine::surface::Create2DTextureParams,
) -> *mut crate::graphics_engine::graphics_engine::HTexture_t {
    unsafe {
        let f: unsafe extern "system" fn(
            device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
            params: *const crate::graphics_engine::surface::Create2DTextureParams,
        ) -> *mut crate::graphics_engine::graphics_engine::HTexture_t = ::std::mem::transmute(
            Create2DTexture_ADDRESS,
        );
        f(device, params)
    }
}
pub const Destroy2DTexture_ADDRESS: usize = 0x141954010;
/// Releases a texture's D3D11 resource and views and frees its handle.
pub unsafe fn Destroy2DTexture(
    device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
    texture: *mut crate::graphics_engine::graphics_engine::HTexture_t,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
            texture: *mut crate::graphics_engine::graphics_engine::HTexture_t,
        ) = ::std::mem::transmute(Destroy2DTexture_ADDRESS);
        f(device, texture)
    }
}
pub const GetRenderTarget_ADDRESS: usize = 0x14195A360;
/// Returns a render-target surface over a slice of `texture`: a texture handle carrying the
/// render-target (or depth-stencil) view for the selected mip level, cube face, and array range.
/// This is the second half of the engine's standard scene-target construction —
/// [`Create2DTexture`](crate::graphics_engine::surface::Create2DTexture) for the storage, then `GetRenderTarget` for the surface bound as a render
/// setup's target.
pub unsafe fn GetRenderTarget(
    device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
    texture: *mut crate::graphics_engine::graphics_engine::HTexture_t,
    mip_level: u32,
    face: u32,
    texarray_start_index: u32,
    texarray_slices: u32,
) -> *mut crate::graphics_engine::graphics_engine::HTexture_t {
    unsafe {
        let f: unsafe extern "system" fn(
            device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
            texture: *mut crate::graphics_engine::graphics_engine::HTexture_t,
            mip_level: u32,
            face: u32,
            texarray_start_index: u32,
            texarray_slices: u32,
        ) -> *mut crate::graphics_engine::graphics_engine::HTexture_t = ::std::mem::transmute(
            GetRenderTarget_ADDRESS,
        );
        f(device, texture, mip_level, face, texarray_start_index, texarray_slices)
    }
}
pub const CreateSurfaceAlias_ADDRESS: usize = 0x14195A630;
/// Creates a second surface over an existing surface's D3D11 resource at a different format: it
/// copies the source handle wholesale, **takes a D3D11 reference on the source resource**, and
/// creates fresh shader-resource, render-target, and depth-stencil views at the requested
/// format. No pixels are copied, and writes through the alias land in the source resource.
///
/// Because the alias holds a reference on the underlying resource, a swapchain buffer that is
/// aliased cannot be resized until the alias is destroyed — `IDXGISwapChain::ResizeBuffers`
/// fails while any reference to buffer 0 outstands.
///
/// When the source is the device back buffer and the requested format already equals the back
/// buffer's format, the shader-resource view is skipped and the alias' own view is left null.
pub unsafe fn CreateSurfaceAlias(
    device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
    params: *const crate::graphics_engine::surface::CreateSurfaceAliasParams,
) -> *mut crate::graphics_engine::graphics_engine::HTexture_t {
    unsafe {
        let f: unsafe extern "system" fn(
            device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
            params: *const crate::graphics_engine::surface::CreateSurfaceAliasParams,
        ) -> *mut crate::graphics_engine::graphics_engine::HTexture_t = ::std::mem::transmute(
            CreateSurfaceAlias_ADDRESS,
        );
        f(device, params)
    }
}
pub const DestroySurface_ADDRESS: usize = 0x1419539C0;
/// Releases a surface's D3D11 views and frees its handle. In this build no active/inactive
/// bookkeeping survives, so destroying a surface twice is an unguarded use-after-free rather
/// than a trapped assertion.
pub unsafe fn DestroySurface(
    device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
    surface: *mut crate::graphics_engine::graphics_engine::HTexture_t,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
            surface: *mut crate::graphics_engine::graphics_engine::HTexture_t,
        ) = ::std::mem::transmute(DestroySurface_ADDRESS);
        f(device, surface)
    }
}
pub const CreateRenderSetup_ADDRESS: usize = 0x1419545F0;
/// Creates a render setup, the render-target configuration a pass binds with
/// [`SetRenderSetup`](crate::graphics_engine::graphics_engine::SetRenderSetup): it allocates the
/// `0xF8`-byte setup, copies the depth target and the three eight-entry target/UAV arrays
/// verbatim, and derives each array's count by scanning for the first null. It reads no width or
/// height — the setup's dimensions come entirely from its bound targets, which is why
/// `SetRenderSetup` can synthesize a full-target viewport from them.
pub unsafe fn CreateRenderSetup(
    device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
    params: *const crate::graphics_engine::surface::CreateRenderSetupParams,
) -> *mut crate::graphics_engine::graphics_engine::HRenderSetup_t {
    unsafe {
        let f: unsafe extern "system" fn(
            device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
            params: *const crate::graphics_engine::surface::CreateRenderSetupParams,
        ) -> *mut crate::graphics_engine::graphics_engine::HRenderSetup_t = ::std::mem::transmute(
            CreateRenderSetup_ADDRESS,
        );
        f(device, params)
    }
}
pub const DestroyRenderSetup_ADDRESS: usize = 0x1419547F0;
/// Frees a render setup. If it is the device's currently-bound setup it is unbound first (via
/// `SetRenderSetup(context, null, false)`), so destroying a bound setup is safe. The targets the
/// setup referenced are not touched.
pub unsafe fn DestroyRenderSetup(
    device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
    setup: *mut crate::graphics_engine::graphics_engine::HRenderSetup_t,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
            setup: *mut crate::graphics_engine::graphics_engine::HRenderSetup_t,
        ) = ::std::mem::transmute(DestroyRenderSetup_ADDRESS);
        f(device, setup)
    }
}
