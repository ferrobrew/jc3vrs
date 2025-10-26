#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct GraphicsEngine {
    _field_0: [u8; 24],
    pub m_CPUFinishedDrawingEvent: u32,
    _field_1c: [u8; 3732],
    pub m_Device: *mut crate::graphics_engine::device::Device,
    _field_eb8: [u8; 4184],
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
