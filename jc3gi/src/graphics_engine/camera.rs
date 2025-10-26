#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct Camera {
    m_OrthoValues: crate::types::math::Vector2,
    m_OffCenterTiles: i32,
    m_OffCenterTileX: i32,
    m_OffCenterTileY: i32,
    m_PreviousTransformF: crate::types::math::Matrix4,
    m_TransformF: crate::types::math::Matrix4,
    m_TransformT0: crate::types::math::Matrix4,
    m_TransformT1: crate::types::math::Matrix4,
    m_ShakeTransform: crate::types::math::Matrix4,
    m_ProjectionF: crate::types::math::Matrix4,
    m_ViewProjectionF: crate::types::math::Matrix4,
    m_PreviousProjF: crate::types::math::Matrix4,
    m_PreviousViewF: crate::types::math::Matrix4,
    m_PreviousViewProjectionF: crate::types::math::Matrix4,
    m_Projection: crate::types::math::Matrix4,
    m_View: crate::types::math::Matrix4,
    m_ViewProjection: crate::types::math::Matrix4,
    m_PreviousProj: crate::types::math::Matrix4,
    m_PreviousView: crate::types::math::Matrix4,
    m_PreviousViewProjection: crate::types::math::Matrix4,
    m_FrustumPlane: [crate::types::math::Plane; 6],
    m_AABNormal: [crate::types::math::Vector4; 12],
    m_Distance: [f32; 6],
    m_ClosestCorner: [crate::graphics_engine::camera::Corner; 6],
    m_StateBitfield: crate::graphics_engine::camera::CameraState,
    _field_55f: [u8; 1],
    m_ConePos: crate::types::math::Vector3,
    m_ConeAxis: crate::types::math::Vector3,
    m_ConeAngleOuterTan: f32,
    m_ConeAngleOuterCosReci: f32,
    m_FOVT0: f32,
    m_FOVT1: f32,
    m_FOV: f32,
    m_FOVProjFactor: f32,
    m_FOVFactor: f32,
    m_Near: f32,
    m_Far: f32,
    m_FactorR: f32,
    m_FactorU: f32,
    m_AspectRatio: f32,
    m_Width: i32,
    m_Height: i32,
}
fn _Camera_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x5B0], Camera>([0u8; 0x5B0]);
    }
    unreachable!()
}
impl Camera {}
impl std::convert::AsRef<Camera> for Camera {
    fn as_ref(&self) -> &Camera {
        self
    }
}
impl std::convert::AsMut<Camera> for Camera {
    fn as_mut(&mut self) -> &mut Camera {
        self
    }
}
#[derive(Copy, Clone, Default)]
#[repr(C, align(1))]
pub struct CameraState {
    /// __int8 m_UseOffCenter : 1;
    /// __int8 m_ScreenshotSeriesRunning : 1;
    /// __int8 m_Ortho : 1;
    /// __int8 m_ComputeView : 1;
    /// __int8 m_DirtyProjection : 1;
    /// __int8 m_IsRenderCamera : 1;
    pub bitfield: u8,
}
fn _CameraState_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1], CameraState>([0u8; 0x1]);
    }
    unreachable!()
}
impl CameraState {}
impl std::convert::AsRef<CameraState> for CameraState {
    fn as_ref(&self) -> &CameraState {
        self
    }
}
impl std::convert::AsMut<CameraState> for CameraState {
    fn as_mut(&mut self) -> &mut CameraState {
        self
    }
}
#[derive(Copy, Clone, Default)]
#[repr(C, align(1))]
pub struct Corner {
    pub x: u8,
    pub y: u8,
    pub z: u8,
}
fn _Corner_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x3], Corner>([0u8; 0x3]);
    }
    unreachable!()
}
impl Corner {}
impl std::convert::AsRef<Corner> for Corner {
    fn as_ref(&self) -> &Corner {
        self
    }
}
impl std::convert::AsMut<Corner> for Corner {
    fn as_mut(&mut self) -> &mut Corner {
        self
    }
}
