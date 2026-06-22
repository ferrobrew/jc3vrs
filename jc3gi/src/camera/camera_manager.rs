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
    /// Sets the active camera's transform, writing both T0 and T1 to `mat` (so the subsequent
    /// interpolation is constant).
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
    /// Sets the active camera's field of view (both T0 and T1).
    pub unsafe fn InitFOV(&mut self, fov: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, fov: f32) = ::std::mem::transmute(
                Self::InitFOV_ADDRESS,
            );
            f(self as *mut Self as _, fov)
        }
    }
    pub const GetRenderCamera_ADDRESS: usize = 0x14009D380;
    /// Returns m_RenderCamera (the camera the render thread reads).
    pub unsafe fn GetRenderCamera(&self) -> *const crate::camera::camera::Camera {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
            ) -> *const crate::camera::camera::Camera = ::std::mem::transmute(
                Self::GetRenderCamera_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const UpdateRender_ADDRESS: usize = 0x1400D4000;
    /// Sim-path per-frame update: iterates every camera in the manager's list and calls
    /// Camera::UpdateRender on each. This is where m_ActiveCamera's m_View is produced for the frame.
    pub unsafe fn UpdateRender(&mut self, dt: f32, dtf: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32, dtf: f32) = ::std::mem::transmute(
                Self::UpdateRender_ADDRESS,
            );
            f(self as *mut Self as _, dt, dtf)
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
