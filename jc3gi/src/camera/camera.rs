#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct Camera {
    pub m_OrthoValues: crate::types::math::Vector2,
    pub m_OffCenterTiles: i32,
    pub m_OffCenterTileX: i32,
    pub m_OffCenterTileY: i32,
    pub m_PreviousTransformF: crate::types::math::Matrix4,
    pub m_TransformF: crate::types::math::Matrix4,
    pub m_TransformT0: crate::types::math::Matrix4,
    pub m_TransformT1: crate::types::math::Matrix4,
    pub m_ShakeTransform: crate::types::math::Matrix4,
    pub m_ProjectionF: crate::types::math::Matrix4,
    pub m_ViewProjectionF: crate::types::math::Matrix4,
    pub m_PreviousProjF: crate::types::math::Matrix4,
    pub m_PreviousViewF: crate::types::math::Matrix4,
    pub m_PreviousViewProjectionF: crate::types::math::Matrix4,
    pub m_Projection: crate::types::math::Matrix4,
    pub m_View: crate::types::math::Matrix4,
    pub m_ViewProjection: crate::types::math::Matrix4,
    pub m_PreviousProj: crate::types::math::Matrix4,
    pub m_PreviousView: crate::types::math::Matrix4,
    pub m_PreviousViewProjection: crate::types::math::Matrix4,
    pub m_FrustumPlane: [crate::types::math::Plane; 6],
    pub m_AABNormal: [crate::types::math::Vector4; 12],
    pub m_Distance: [f32; 6],
    pub m_ClosestCorner: [crate::camera::camera::Corner; 6],
    pub m_StateBitfield: crate::camera::camera::CameraState,
    _field_55f: [u8; 1],
    pub m_ConePos: crate::types::math::Vector3,
    pub m_ConeAxis: crate::types::math::Vector3,
    pub m_ConeAngleOuterTan: f32,
    pub m_ConeAngleOuterCosReci: f32,
    pub m_FOVT0: f32,
    pub m_FOVT1: f32,
    pub m_FOV: f32,
    pub m_FOVProjFactor: f32,
    pub m_FOVFactor: f32,
    pub m_Near: f32,
    pub m_Far: f32,
    pub m_FactorR: f32,
    pub m_FactorU: f32,
    pub m_AspectRatio: f32,
    pub m_Width: i32,
    pub m_Height: i32,
}
fn _Camera_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x5B0], Camera>([0u8; 0x5B0]);
    }
    unreachable!()
}
impl Camera {
    pub const UpdateRender_ADDRESS: usize = 0x1400C3020;
    /// The per-frame camera update: snapshots the previous view-projection, lerps the transform from
    /// `m_TransformT0` to `m_TransformT1` by `dtf`, and recalculates the projection and frustum.
    pub unsafe fn UpdateRender(&mut self, dt: f32, dtf: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32, dtf: f32) = ::std::mem::transmute(
                Self::UpdateRender_ADDRESS,
            );
            f(self as *mut Self as _, dt, dtf)
        }
    }
    pub const SetupRenderCamera_ADDRESS: usize = 0x1400B3B80;
    /// Builds the render-camera matrices once per draw: applies reverse-Z and the TAA jitter to the
    /// projection, then rebuilds `m_ViewProjection` and `m_ViewProjectionF` from `m_View`. Runs at most
    /// once per camera, guarded by the `m_IsRenderCamera` flag. Returns the last matrix-copy
    /// destination.
    pub unsafe fn SetupRenderCamera(&mut self, jitter: bool) -> *mut ::std::ffi::c_void {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                jitter: bool,
            ) -> *mut ::std::ffi::c_void = ::std::mem::transmute(
                Self::SetupRenderCamera_ADDRESS,
            );
            f(self as *mut Self as _, jitter)
        }
    }
    pub const SetComputeView_ADDRESS: usize = 0x14009BDB0;
    /// Sets or clears the `m_ComputeView` flag. When set, [`UpdateRender`](Camera::UpdateRender)
    /// re-derives `m_View` as the inverse of `m_TransformF` each frame instead of reading a supplied
    /// view matrix.
    pub unsafe fn SetComputeView(&mut self, enable: bool) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, enable: bool) = ::std::mem::transmute(
                Self::SetComputeView_ADDRESS,
            );
            f(self as *mut Self as _, enable)
        }
    }
    pub const RecalcProjection_ADDRESS: usize = 0x1400B24F0;
    /// Rebuilds `m_Projection` from the FOV, near, far, and aspect parameters (perspective,
    /// off-center, or orthographic per the `m_UseOffCenter` and `m_Ortho` flags), clears
    /// `m_DirtyProjection`, and applies the reverse-Z remap only when `m_IsRenderCamera` is set.
    pub unsafe fn RecalcProjection(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::RecalcProjection_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
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
crate::__bitflags! {
    pub struct CameraState : u8 { const m_UseOffCenter = 1usize as _; const
    m_ScreenshotSeriesRunning = 2usize as _; const m_Ortho = 4usize as _; const
    m_ComputeView = 8usize as _; const m_DirtyProjection = 16usize as _; const
    m_IsRenderCamera = 32usize as _; }
}
fn _CameraState_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1], CameraState>([0u8; 0x1]);
    }
    unreachable!()
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
