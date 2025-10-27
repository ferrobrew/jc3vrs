#![allow(
    dead_code,
    non_snake_case,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct CameraContext {
    pub m_CameraTransform: crate::types::math::Matrix4,
    pub m_AlternateAimTransform: crate::types::math::Matrix4,
    pub m_ListenerTransform: crate::types::math::Matrix4,
    pub m_FOV: f32,
    pub m_DOFParameters: crate::camera::camera_context::CameraDOFParameters,
    pub m_MotionBlurShutterExposure: f32,
    pub m_MaxMotionBlur: f32,
    pub m_MotionBlurFactor: f32,
    pub m_RadialBlurFactor: f32,
    pub m_RadialBlurOffset: f32,
    pub m_RadialBlurPosX: f32,
    pub m_RadialBlurPosY: f32,
    pub m_ExposureValue: f32,
    pub m_BloomLighten: f32,
    pub m_BloomContrast: f32,
    pub m_BloomSecontaryLighten: f32,
    pub m_BloomThreshold: f32,
    pub m_ColorTint: crate::types::math::Vector3,
    pub m_ColorTintCurveTexWeight: f32,
    pub m_VignetteIndex: i32,
    _field_124: [u8; 4],
    pub m_ShadowFocusAlias: u64,
    pub m_ColorTintCurveTexturePath: u32,
    _field_134: [u8; 4],
    pub m_ShadowRadiusNear: f32,
    pub m_ShadowRadiusMedium: f32,
    pub m_ShadowRadiusFar: f32,
    pub m_ShadowFocusOffsetNear: f32,
    pub m_ShadowFocusOffsetMedium: f32,
    pub m_ShadowFocusOffsetFar: f32,
}
fn _CameraContext_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x150], CameraContext>([0u8; 0x150]);
    }
    unreachable!()
}
impl CameraContext {}
impl std::convert::AsRef<CameraContext> for CameraContext {
    fn as_ref(&self) -> &CameraContext {
        self
    }
}
impl std::convert::AsMut<CameraContext> for CameraContext {
    fn as_mut(&mut self) -> &mut CameraContext {
        self
    }
}
#[repr(C, align(8))]
pub struct CameraControlContext {
    pub m_Dt: f32,
    pub m_RenderDt: f32,
    pub m_RenderDtf: f32,
    pub m_BlendDt: f32,
    pub m_CAMERA_DT_EPSILON: f32,
    _field_14: [u8; 12],
    pub m_PreviousCameraContext: crate::camera::camera_context::CameraContext,
    pub m_NextCameraContext: crate::camera::camera_context::CameraContext,
    pub m_PreviousRenderContext: crate::camera::camera_context::CameraContext,
    pub m_NextRenderContext: crate::camera::camera_context::CameraContext,
    pub m_PresetWeights: crate::camera::camera_context::CameraEnvironmentPresetWeights,
    pub m_ActivePipelines: [u8; 32],
}
fn _CameraControlContext_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x5D0], CameraControlContext>([0u8; 0x5D0]);
    }
    unreachable!()
}
impl CameraControlContext {}
impl std::convert::AsRef<CameraControlContext> for CameraControlContext {
    fn as_ref(&self) -> &CameraControlContext {
        self
    }
}
impl std::convert::AsMut<CameraControlContext> for CameraControlContext {
    fn as_mut(&mut self) -> &mut CameraControlContext {
        self
    }
}
#[repr(C, align(4))]
pub struct CameraDOFParameters {
    pub m_FocalDistanceNear: f32,
    pub m_FocalDistanceFar: f32,
    pub m_MaxDOF: f32,
    pub m_MaxDOFNear: f32,
    pub m_DOFSmoothness: f32,
    pub m_DOFSmoothnessNear: f32,
    pub m_DOFHeightFalloff: f32,
}
fn _CameraDOFParameters_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1C], CameraDOFParameters>([0u8; 0x1C]);
    }
    unreachable!()
}
impl CameraDOFParameters {}
impl std::convert::AsRef<CameraDOFParameters> for CameraDOFParameters {
    fn as_ref(&self) -> &CameraDOFParameters {
        self
    }
}
impl std::convert::AsMut<CameraDOFParameters> for CameraDOFParameters {
    fn as_mut(&mut self) -> &mut CameraDOFParameters {
        self
    }
}
#[repr(C, align(8))]
pub struct CameraEnvironmentPresetWeights {
    pub m_DOFParameters: crate::camera::camera_context::CameraDOFParameters,
    pub m_MotionBlurShutterExposureWeight: f32,
    pub m_MotionBlurFactorWeight: f32,
    pub m_MaxMotionBlurWeight: f32,
    pub m_RadialBlurFactorWeight: f32,
    pub m_RadialBlurOffsetWeight: f32,
    pub m_RadialBlurPosXWeight: f32,
    pub m_RadialBlurPosYWeight: f32,
    pub m_BloomContrastWeight: f32,
    pub m_BloomLightenWeight: f32,
    pub m_BloomSecontaryLightenWeight: f32,
    pub m_BloomThresholdWeight: f32,
    pub m_ColorTintWeight: f32,
    pub m_ColorTintCurveTexWeight: f32,
}
fn _CameraEnvironmentPresetWeights_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x50], CameraEnvironmentPresetWeights>([0u8; 0x50]);
    }
    unreachable!()
}
impl CameraEnvironmentPresetWeights {}
impl std::convert::AsRef<CameraEnvironmentPresetWeights>
for CameraEnvironmentPresetWeights {
    fn as_ref(&self) -> &CameraEnvironmentPresetWeights {
        self
    }
}
impl std::convert::AsMut<CameraEnvironmentPresetWeights>
for CameraEnvironmentPresetWeights {
    fn as_mut(&mut self) -> &mut CameraEnvironmentPresetWeights {
        self
    }
}
