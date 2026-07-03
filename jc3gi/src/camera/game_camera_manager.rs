#![cfg_attr(any(), rustfmt::skip)]
#[allow(unused_imports)]
use crate::{camera::camera_manager::CameraManager, camera::camera_tree::CameraTree};
#[repr(C, align(8))]
/// An opaque node in the camera pipeline.
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
    /// Funnels the camera control contexts into the active camera state: reads the next render
    /// context, sets the audio listener, applies the jitter filter, and calls
    /// [`CameraManager::InitTransform`] and [`CameraManager::InitFOV`].
    pub unsafe fn PushRenderContext(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::PushRenderContext_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const UpdateBlackboardValues_ADDRESS: usize = 0x1407FFF90;
    /// Reads the action-effector inputs into the transformed-gamepad-input blackboard value.
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
    /// Whether the currently selected camera in the director stack has the cinematic flag set.
    pub unsafe fn IsInCinematicCamera(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsInCinematicCamera_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsAlternateAimTransformUsed_ADDRESS: usize = 0x14075C820;
    /// Whether the alternate-aim (aim-down-sights) transform is in use.
    pub unsafe fn IsAlternateAimTransformUsed(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsAlternateAimTransformUsed_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const GetInputMatrix_ADDRESS: usize = 0x14075C7A0;
    /// Writes the camera matrix used for mapping player input to world space — the same matrix the
    /// locomotion input task reads to make the move direction camera-relative (its negated third
    /// row is the camera forward on the ground plane). Reads
    /// [`CameraControlContext::m_NextRenderContext`](camera::camera_context::CameraControlContext::m_NextRenderContext)'s camera transform.
    pub unsafe fn GetInputMatrix(&self, matrix: *mut crate::types::math::Matrix4) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                matrix: *mut crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::GetInputMatrix_ADDRESS);
            f(self as *const Self as _, matrix)
        }
    }
    pub const GetCameraMatrix_ADDRESS: usize = 0x14075C7C0;
    /// Writes the sim-phase camera matrix: [`CameraControlContext::m_NextCameraContext`](camera::camera_context::CameraControlContext::m_NextCameraContext)'s camera
    /// transform, as opposed to [`GetInputMatrix`](GameCameraManager::GetInputMatrix)'s next *render* context. Read by the player aim
    /// control (raycast start position, adjusted camera matrix, target visibility casts), weapon
    /// aim-target queries, melee and grapple state tasks, and other sim-side camera consumers.
    pub unsafe fn GetCameraMatrix(&self, matrix: *mut crate::types::math::Matrix4) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                matrix: *mut crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::GetCameraMatrix_ADDRESS);
            f(self as *const Self as _, matrix)
        }
    }
    pub const GetAlternateAimMatrix_ADDRESS: usize = 0x14075C830;
    /// Writes the alternate-aim (aim-down-sights) matrix:
    /// [`CameraControlContext::m_NextRenderContext`](camera::camera_context::CameraControlContext::m_NextRenderContext)'s alternate aim transform. The player aim
    /// control prefers it over [`GetCameraMatrix`](GameCameraManager::GetCameraMatrix) when [`IsAlternateAimTransformUsed`](GameCameraManager::IsAlternateAimTransformUsed) is set.
    pub unsafe fn GetAlternateAimMatrix(
        &self,
        matrix: *mut crate::types::math::Matrix4,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                matrix: *mut crate::types::math::Matrix4,
            ) = ::std::mem::transmute(Self::GetAlternateAimMatrix_ADDRESS);
            f(self as *const Self as _, matrix)
        }
    }
    pub const UpdateRender_ADDRESS: usize = 0x1407F4560;
    /// The per-frame render update: runs the camera tree via
    /// [`CameraTree::UpdateRenderContexts`], pushes the render context, then updates the lighting and
    /// water level at the camera.
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
