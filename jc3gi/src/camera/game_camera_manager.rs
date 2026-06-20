#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A node in the camera pipeline (opaque).
pub struct CameraPipeline {}
impl CameraPipeline {}
impl std::convert::AsRef<CameraPipeline> for CameraPipeline {
    fn as_ref(&self) -> &CameraPipeline {
        self
    }
}
impl std::convert::AsMut<CameraPipeline> for CameraPipeline {
    fn as_mut(&mut self) -> &mut CameraPipeline {
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
    /// Funnels the camera control contexts into the active camera state: reads m_NextRenderContext,
    /// sets the audio listener, applies the jitter filter, and calls CameraManager InitTransform/InitFOV.
    pub unsafe fn PushRenderContext(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::PushRenderContext_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const UpdateBlackboardValues_ADDRESS: usize = 0x1407FFF90;
    /// Reads the action-effector inputs into m_TransformedGamepadInput.
    pub unsafe fn UpdateBlackboardValues(
        &mut self,
        pipeline: *const crate::camera::game_camera_manager::CameraPipeline,
        dt: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                pipeline: *const crate::camera::game_camera_manager::CameraPipeline,
                dt: f32,
            ) = ::std::mem::transmute(Self::UpdateBlackboardValues_ADDRESS);
            f(self as *mut Self as _, pipeline, dt)
        }
    }
    pub const IsInCinematicCamera_ADDRESS: usize = 0x14075C850;
    /// True when the currently selected camera in the director stack has the cinematic flag set.
    pub unsafe fn IsInCinematicCamera(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsInCinematicCamera_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsAlternateAimTransformUsed_ADDRESS: usize = 0x14075C820;
    /// True when the alternate-aim (ADS) transform is in use.
    pub unsafe fn IsAlternateAimTransformUsed(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsAlternateAimTransformUsed_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const UpdateRender_ADDRESS: usize = 0x1407F4560;
    /// Per-frame render update: runs the camera tree (UpdateRenderContexts), pushes the render
    /// context, then updates the lighting and water level at the camera.
    pub unsafe fn UpdateRender(&mut self, dt: f32, dtf: f32, blend: f32) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                dtf: f32,
                blend: f32,
            ) -> u64 = ::std::mem::transmute(Self::UpdateRender_ADDRESS);
            f(self as *mut Self as _, dt, dtf, blend)
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
