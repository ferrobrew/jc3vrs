#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::HICON};
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
pub enum ActiveCursor {
    None = -1isize as _,
    Arrow = 0isize as _,
    Cross = 1isize as _,
    Slider = 2isize as _,
    Zoom = 3isize as _,
}
fn _ActiveCursor_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], ActiveCursor>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
pub struct GraphicsEngine {
    _field_0: [u8; 24],
    pub m_CPUFinishedDrawingEvent: u32,
    _field_1c: [u8; 268],
    pub m_ActiveCursor: crate::graphics_engine::graphics_engine::ActiveCursor,
    _field_12c: [u8; 3460],
    pub m_Device: *mut crate::graphics_engine::device::Device,
    _field_eb8: [u8; 888],
    pub m_BackBufferLinear: *mut crate::graphics_engine::texture::Texture,
    _field_1238: [u8; 3288],
}
fn _GraphicsEngine_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1F10], GraphicsEngine>([0u8; 0x1F10]);
    }
    unreachable!()
}
impl GraphicsEngine {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417121520usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl GraphicsEngine {}
impl std::convert::AsRef<GraphicsEngine> for GraphicsEngine {
    fn as_ref(&self) -> &GraphicsEngine {
        self
    }
}
impl std::convert::AsMut<GraphicsEngine> for GraphicsEngine {
    fn as_mut(&mut self) -> &mut GraphicsEngine {
        self
    }
}
#[repr(C, align(8))]
pub struct GraphicsParams {
    pub m_AppTitle: *const u8,
    pub m_Cursors: [crate::graphics_engine::graphics_engine::HICON; 4],
    pub m_Hwnd: crate::graphics_engine::graphics_engine::HWND,
    pub m_FullscreenWidth: i32,
    pub m_FullscreenHeight: i32,
    pub m_WindowedWidth: i32,
    pub m_WindowedHeight: i32,
    pub m_Fullscreen: bool,
    pub m_HighResShadows: bool,
    _field_42: [u8; 2],
    pub m_Width: u32,
    pub m_Height: u32,
    pub m_IsHighDef: bool,
    _field_4d: [u8; 3],
    pub m_DisplayPresentationInterval: u32,
    pub m_RendertargetCount: u32,
    pub m_RefreshRate: u16,
    _field_5a: [u8; 22],
}
fn _GraphicsParams_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x70], GraphicsParams>([0u8; 0x70]);
    }
    unreachable!()
}
impl GraphicsParams {}
impl std::convert::AsRef<GraphicsParams> for GraphicsParams {
    fn as_ref(&self) -> &GraphicsParams {
        self
    }
}
impl std::convert::AsMut<GraphicsParams> for GraphicsParams {
    fn as_mut(&mut self) -> &mut GraphicsParams {
        self
    }
}
pub unsafe fn get_graphics_params() -> &'static mut crate::graphics_engine::graphics_engine::GraphicsParams {
    unsafe {
        &mut *(0x142D3A850
            as *mut crate::graphics_engine::graphics_engine::GraphicsParams)
    }
}
