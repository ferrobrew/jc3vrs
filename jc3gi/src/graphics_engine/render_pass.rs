#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct CConstantBufferPool {}
impl CConstantBufferPool {
    pub const HandBackBuffers_ADDRESS: usize = 0x1400E04F0;
    pub unsafe fn HandBackBuffers(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::HandBackBuffers_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<CConstantBufferPool> for CConstantBufferPool {
    fn as_ref(&self) -> &CConstantBufferPool {
        self
    }
}
impl std::convert::AsMut<CConstantBufferPool> for CConstantBufferPool {
    fn as_mut(&mut self) -> &mut CConstantBufferPool {
        self
    }
}
#[repr(C, align(8))]
pub struct CRenderPass {}
impl CRenderPass {
    pub const SetupRenderFrameData_ADDRESS: usize = 0x14048C4E0;
    pub unsafe fn SetupRenderFrameData() {
        unsafe {
            let f: unsafe extern "system" fn() = ::std::mem::transmute(
                Self::SetupRenderFrameData_ADDRESS,
            );
            f()
        }
    }
}
impl std::convert::AsRef<CRenderPass> for CRenderPass {
    fn as_ref(&self) -> &CRenderPass {
        self
    }
}
impl std::convert::AsMut<CRenderPass> for CRenderPass {
    fn as_mut(&mut self) -> &mut CRenderPass {
        self
    }
}
