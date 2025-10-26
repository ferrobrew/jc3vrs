#![allow(
    dead_code,
    non_snake_case,
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
impl CameraManager {}
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
