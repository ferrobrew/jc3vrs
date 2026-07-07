#![cfg_attr(any(), rustfmt::skip)]
pub use windows::Win32::System::Threading::CRITICAL_SECTION as CRITICAL_SECTION;
#[repr(C, align(8))]
pub struct Context {
    _field_0: [u8; 32800],
    pub m_Context: crate::graphics_engine::device::ID3D11DeviceContext,
    pub m_Mutex: *mut crate::graphics_engine::device::CRITICAL_SECTION,
    _field_8030: [u8; 3264],
}
fn _Context_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8CF0], Context>([0u8; 0x8CF0]);
    }
    unreachable!()
}
impl Context {}
impl std::convert::AsRef<Context> for Context {
    fn as_ref(&self) -> &Context {
        self
    }
}
impl std::convert::AsMut<Context> for Context {
    fn as_mut(&mut self) -> &mut Context {
        self
    }
}
#[repr(C, align(8))]
pub struct Device {
    pub m_Context: *mut crate::graphics_engine::device::Context,
    /// The presentable back-buffer surface owned by the DXGI swapchain, returned by
    /// `GetDeviceSurface``(BackBuffer)`. `ResizeBuffers` recreates its RTV/SRV/texture on a
    /// swapchain resize.
    pub m_BackBuffer: *mut crate::graphics_engine::texture::Texture,
    _field_10: [u8; 16],
    pub m_SwapChain: crate::graphics_engine::device::IDXGISwapChain,
    pub m_Device: crate::graphics_engine::device::ID3D11Device,
    _field_30: [u8; 8],
    pub m_DXGIOutput: crate::graphics_engine::device::IDXGIOutput,
    _field_40: [u8; 336],
    /// The device's live mode descriptor. `ResizeBuffers` writes its display dimensions/ratio on a
    /// swapchain resize; `GetDeviceInfo` returns a copy of it. Subsystems that re-size their own
    /// render targets read their dimensions from here (via `GetDeviceInfo`), so it is the
    /// device-side source of the display size independent of the [`CreateRenderSetups`](graphics_engine::graphics_engine::GraphicsEngine::CreateRenderSetups)
    /// parameter.
    pub m_DeviceInfo: crate::graphics_engine::device::DeviceInfo,
    _field_1c8: [u8; 88],
    /// Set when the swapchain has been resized since the last time the flag was consumed.
    pub m_WasResized: bool,
    _field_221: [u8; 35679],
}
fn _Device_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8D80], Device>([0u8; 0x8D80]);
    }
    unreachable!()
}
impl Device {}
impl std::convert::AsRef<Device> for Device {
    fn as_ref(&self) -> &Device {
        self
    }
}
impl std::convert::AsMut<Device> for Device {
    fn as_mut(&mut self) -> &mut Device {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(8))]
/// The device's cached mode/capability descriptor. `GetDeviceInfo` returns a copy of the device's own
/// instance ([`Device::m_DeviceInfo`]), and [`CreateRenderSetups`](graphics_engine::graphics_engine::GraphicsEngine::CreateRenderSetups)
/// sizes every scene render target from a caller-supplied copy's
/// [`m_DisplayWidth`](DeviceInfo::m_DisplayWidth)/[`m_DisplayHeight`](DeviceInfo::m_DisplayHeight).
pub struct DeviceInfo {
    _field_0: [u8; 16],
    /// The display width in pixels. The source of every scene render target's width.
    pub m_DisplayWidth: u32,
    /// The display height in pixels. The source of every scene render target's height.
    pub m_DisplayHeight: u32,
    /// The display aspect ratio (`m_DisplayWidth / m_DisplayHeight`).
    pub m_DisplayRatio: f32,
    /// The present sync interval (0 disables vsync).
    pub m_DisplayPresentationInterval: u32,
    _field_20: [u8; 24],
}
fn _DeviceInfo_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x38], DeviceInfo>([0u8; 0x38]);
    }
    unreachable!()
}
impl DeviceInfo {}
impl std::convert::AsRef<DeviceInfo> for DeviceInfo {
    fn as_ref(&self) -> &DeviceInfo {
        self
    }
}
impl std::convert::AsMut<DeviceInfo> for DeviceInfo {
    fn as_mut(&mut self) -> &mut DeviceInfo {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The surface a device exposes to `GetDeviceSurface`. The presentable back buffer is
/// [`BackBuffer`](DeviceSurface::BackBuffer); the eye-specific variants are for stereo output devices.
pub enum DeviceSurface {
    FrontBuffer = 0isize as _,
    BackBuffer = 1isize as _,
    SecondBackBuffer = 2isize as _,
    FrontBufferLeftEye = 3isize as _,
    FrontBufferRightEye = 4isize as _,
    BackBufferLeftEye = 5isize as _,
    BackBufferRightEye = 6isize as _,
    SecondBackBufferLeftEye = 7isize as _,
    SecondBackBufferRightEye = 8isize as _,
}
fn _DeviceSurface_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], DeviceSurface>([0u8; 0x4]);
    }
    unreachable!()
}
pub use windows::Win32::Graphics::Direct3D11::ID3D11Device as ID3D11Device;
pub use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext as ID3D11DeviceContext;
pub use windows::Win32::Graphics::Dxgi::IDXGIOutput as IDXGIOutput;
pub use windows::Win32::Graphics::Dxgi::IDXGISwapChain as IDXGISwapChain;
pub const GetMasterContext_ADDRESS: usize = 0x1419550D0;
/// Returns the master context, the wrapper around the D3D11 immediate context. `device` is unused.
pub unsafe fn GetMasterContext(
    this: *mut ::std::ffi::c_void,
    device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
) -> *mut crate::graphics_engine::device::Context {
    unsafe {
        let f: unsafe extern "system" fn(
            this: *mut ::std::ffi::c_void,
            device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
        ) -> *mut crate::graphics_engine::device::Context = ::std::mem::transmute(
            GetMasterContext_ADDRESS,
        );
        f(this, device)
    }
}
pub const GetDeviceInfo_ADDRESS: usize = 0x1419525F0;
/// Copies the device's [`m_DeviceInfo`](Device::m_DeviceInfo) into `out` (a plain `memcpy` of the
/// `0x38`-byte descriptor).
pub unsafe fn GetDeviceInfo(
    device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
    out: *mut crate::graphics_engine::device::DeviceInfo,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
            out: *mut crate::graphics_engine::device::DeviceInfo,
        ) = ::std::mem::transmute(GetDeviceInfo_ADDRESS);
        f(device, out)
    }
}
pub const GetDeviceSurface_ADDRESS: usize = 0x141956260;
/// Returns one of the device's presentable surfaces. Only [`BackBuffer`](DeviceSurface::BackBuffer)
/// resolves (to [`Device::m_BackBuffer`]); the other selectors return null on this non-stereo device.
/// [`CreateRenderSetups`](graphics_engine::graphics_engine::GraphicsEngine::CreateRenderSetups) uses it
/// to alias `BackBufferLinear` and the back-buffer render setups onto the live swapchain surface.
pub unsafe fn GetDeviceSurface(
    device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
    surface: crate::graphics_engine::device::DeviceSurface,
) -> *mut crate::graphics_engine::texture::Texture {
    unsafe {
        let f: unsafe extern "system" fn(
            device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
            surface: crate::graphics_engine::device::DeviceSurface,
        ) -> *mut crate::graphics_engine::texture::Texture = ::std::mem::transmute(
            GetDeviceSurface_ADDRESS,
        );
        f(device, surface)
    }
}
pub const ResizeBuffers_ADDRESS: usize = 0x141952400;
/// Resizes the DXGI swapchain's buffers. Unbinds render targets, releases and recreates the back
/// buffer's texture/RTV/SRV via `IDXGISwapChain::ResizeBuffers`, and updates
/// [`Device::m_DeviceInfo`]'s display dimensions and [`Device::m_WasResized`]. This is the swapchain
/// half of a resize, distinct from re-creating the scene render targets in
/// [`CreateRenderSetups`](graphics_engine::graphics_engine::GraphicsEngine::CreateRenderSetups).
pub unsafe fn ResizeBuffers(
    device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
    width: u32,
    height: u32,
) -> bool {
    unsafe {
        let f: unsafe extern "system" fn(
            device: *mut crate::graphics_engine::graphics_engine::HDevice_t,
            width: u32,
            height: u32,
        ) -> bool = ::std::mem::transmute(ResizeBuffers_ADDRESS);
        f(device, width, height)
    }
}
