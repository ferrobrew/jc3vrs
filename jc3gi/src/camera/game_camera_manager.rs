#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct CCameraPipeline {}
impl CCameraPipeline {}
impl std::convert::AsRef<CCameraPipeline> for CCameraPipeline {
    fn as_ref(&self) -> &CCameraPipeline {
        self
    }
}
impl std::convert::AsMut<CCameraPipeline> for CCameraPipeline {
    fn as_mut(&mut self) -> &mut CCameraPipeline {
        self
    }
}
#[repr(C, align(8))]
pub struct GameCameraManager {
    _field_0: [u8; 224],
    pub m_ControlContext: crate::camera::camera_context::CameraControlContext,
    _field_6b0: [u8; 168],
}
fn _GameCameraManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x758], GameCameraManager>([0u8; 0x758]);
    }
    unreachable!()
}
impl GameCameraManager {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418092208usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl GameCameraManager {
    pub const PushRenderContext_ADDRESS: usize = 0x1407ECB00;
    pub unsafe fn PushRenderContext(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::PushRenderContext_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const UpdateBlackboardValues_ADDRESS: usize = 0x1407FFF90;
    pub unsafe fn UpdateBlackboardValues(
        &mut self,
        pipeline: *const crate::camera::game_camera_manager::CCameraPipeline,
        dt: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                pipeline: *const crate::camera::game_camera_manager::CCameraPipeline,
                dt: f32,
            ) = ::std::mem::transmute(Self::UpdateBlackboardValues_ADDRESS);
            f(self as *mut Self as _, pipeline, dt)
        }
    }
    pub const IsInCinematicCamera_ADDRESS: usize = 0x14075C850;
    pub unsafe fn IsInCinematicCamera(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsInCinematicCamera_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsAlternateAimTransformUsed_ADDRESS: usize = 0x14075C820;
    pub unsafe fn IsAlternateAimTransformUsed(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsAlternateAimTransformUsed_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
}
impl std::convert::AsRef<GameCameraManager> for GameCameraManager {
    fn as_ref(&self) -> &GameCameraManager {
        self
    }
}
impl std::convert::AsMut<GameCameraManager> for GameCameraManager {
    fn as_mut(&mut self) -> &mut GameCameraManager {
        self
    }
}
