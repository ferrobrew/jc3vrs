#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct CUIManager {}
impl CUIManager {
    pub unsafe fn StartRender(
        &mut self,
        context: *const crate::graphics_engine::graphics_engine::HContext_t,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                context: *const crate::graphics_engine::graphics_engine::HContext_t,
            ) = ::std::mem::transmute(0x140F1B030 as usize);
            f(self as *mut Self as _, context)
        }
    }
    pub unsafe fn Submit(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                0x140F1B0D0 as usize,
            );
            f(self as *mut Self as _)
        }
    }
    pub unsafe fn IsUsingStaticBackGround(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                0x140F1B4C0 as usize,
            );
            f(self as *const Self as _)
        }
    }
}
impl std::convert::AsRef<CUIManager> for CUIManager {
    fn as_ref(&self) -> &CUIManager {
        self
    }
}
impl std::convert::AsMut<CUIManager> for CUIManager {
    fn as_mut(&mut self) -> &mut CUIManager {
        self
    }
}
