#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct CameraManager {
    _field_0: [u8; 16],
    pub m_DefaultCamera: crate::camera::camera::Camera,
    pub m_ActiveCamera: *mut crate::camera::camera::Camera,
    pub m_RenderCamera: *mut crate::camera::camera::Camera,
    pub m_AspectRatio: f32,
    _field_5d4: [u8; 20],
}
fn _CameraManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x5E8], CameraManager>([0u8; 0x5E8]);
    }
    unreachable!()
}
impl CameraManager {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417799200usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl CameraManager {
    pub const InitTransform_ADDRESS: usize = 0x14009D390;
    pub unsafe fn InitTransform(&mut self, mat: *const crate::types::math::Matrix4) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                mat: *const crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::InitTransform_ADDRESS);
            f(self as *mut Self as _, mat)
        }
    }
    pub const InitFOV_ADDRESS: usize = 0x14009D400;
    pub unsafe fn InitFOV(&mut self, fov: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, fov: f32) = ::std::mem::transmute(
                Self::InitFOV_ADDRESS,
            );
            f(self as *mut Self as _, fov)
        }
    }
}
impl std::convert::AsRef<CameraManager> for CameraManager {
    fn as_ref(&self) -> &CameraManager {
        self
    }
}
impl std::convert::AsMut<CameraManager> for CameraManager {
    fn as_mut(&mut self) -> &mut CameraManager {
        self
    }
}
