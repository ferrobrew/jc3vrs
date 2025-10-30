#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(1))]
pub struct RenderEngine {
    _field_0: [u8; 8736],
}
fn _RenderEngine_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x2220], RenderEngine>([0u8; 0x2220]);
    }
    unreachable!()
}
impl RenderEngine {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417799192usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl RenderEngine {}
impl std::convert::AsRef<RenderEngine> for RenderEngine {
    fn as_ref(&self) -> &RenderEngine {
        self
    }
}
impl std::convert::AsMut<RenderEngine> for RenderEngine {
    fn as_mut(&mut self) -> &mut RenderEngine {
        self
    }
}
