#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
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
impl RenderEngine {
    pub const PostDraw_ADDRESS: usize = 0x1401C2350;
    pub unsafe fn PostDraw(
        &mut self,
        context: *const crate::graphics_engine::graphics_engine::HContext_t,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                context: *const crate::graphics_engine::graphics_engine::HContext_t,
            ) = ::std::mem::transmute(Self::PostDraw_ADDRESS);
            f(self as *mut Self as _, context)
        }
    }
}
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
