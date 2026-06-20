#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// Root of the camera pipeline tree: flattens and weights the pipelines and runs their render
/// modifiers, populating the render contexts each frame.
pub struct CameraTree {}
impl CameraTree {
    pub const UpdateRenderContexts_ADDRESS: usize = 0x140465AD0;
    pub unsafe fn UpdateRenderContexts(
        &mut self,
        ctx: *mut crate::camera::camera_context::CameraControlContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::camera::camera_context::CameraControlContext,
            ) = ::std::mem::transmute(Self::UpdateRenderContexts_ADDRESS);
            f(self as *mut Self as _, ctx)
        }
    }
}
impl std::convert::AsRef<CameraTree> for CameraTree {
    fn as_ref(&self) -> &CameraTree {
        self
    }
}
impl std::convert::AsMut<CameraTree> for CameraTree {
    fn as_mut(&mut self) -> &mut CameraTree {
        self
    }
}
