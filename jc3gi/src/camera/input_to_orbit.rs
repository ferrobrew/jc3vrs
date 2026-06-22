#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The camera arm/boom; the look delta rotates it.
pub struct BoomTransform {}
impl BoomTransform {
    pub const DeltaTransform_ADDRESS: usize = 0x14043D180;
    /// Apply a (yaw, pitch, roll) delta-angle to the boom.
    pub unsafe fn DeltaTransform(&mut self, yaw: f32, pitch: f32, roll: f32) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                yaw: f32,
                pitch: f32,
                roll: f32,
            ) = ::std::mem::transmute(Self::DeltaTransform_ADDRESS);
            f(self as *mut Self as _, yaw, pitch, roll)
        }
    }
}
impl std::convert::AsRef<BoomTransform> for BoomTransform {
    fn as_ref(&self) -> &BoomTransform {
        self
    }
}
impl std::convert::AsMut<BoomTransform> for BoomTransform {
    fn as_mut(&mut self) -> &mut BoomTransform {
        self
    }
}
#[repr(C, align(8))]
pub struct InputToOrbitModifier {}
impl InputToOrbitModifier {
    pub const CalculateInputDeltaAngles_ADDRESS: usize = 0x1406CB3F0;
    /// Read the look effectors, combine them, apply per-axis sensitivity, and return the
    /// delta-angle (yaw, pitch).
    pub unsafe fn CalculateInputDeltaAngles(
        &mut self,
        params: *const crate::camera::input_to_orbit::SInputToOrbitModifierParams,
        pipeline: *mut crate::camera::camera_context::CameraPipelineContext,
    ) -> crate::types::math::Vector2 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                params: *const crate::camera::input_to_orbit::SInputToOrbitModifierParams,
                pipeline: *mut crate::camera::camera_context::CameraPipelineContext,
            ) -> crate::types::math::Vector2 = ::std::mem::transmute(
                Self::CalculateInputDeltaAngles_ADDRESS,
            );
            f(self as *mut Self as _, params, pipeline)
        }
    }
    pub const ProcessCameraContext_ADDRESS: usize = 0x1406DBB80;
    /// Caller of CalculateInputDeltaAngles: takes the returned delta-angle and applies it to the
    /// camera via BoomTransform::DeltaTransform (the call is at +0x17D).
    pub unsafe fn ProcessCameraContext(
        &mut self,
        pipeline: *mut crate::camera::camera_context::CameraPipelineContext,
        previous: *const crate::camera::camera_context::CameraContext,
        out: *mut crate::camera::camera_context::CameraContext,
    ) -> f64 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                pipeline: *mut crate::camera::camera_context::CameraPipelineContext,
                previous: *const crate::camera::camera_context::CameraContext,
                out: *mut crate::camera::camera_context::CameraContext,
            ) -> f64 = ::std::mem::transmute(Self::ProcessCameraContext_ADDRESS);
            f(self as *mut Self as _, pipeline, previous, out)
        }
    }
}
impl std::convert::AsRef<InputToOrbitModifier> for InputToOrbitModifier {
    fn as_ref(&self) -> &InputToOrbitModifier {
        self
    }
}
impl std::convert::AsMut<InputToOrbitModifier> for InputToOrbitModifier {
    fn as_mut(&mut self) -> &mut InputToOrbitModifier {
        self
    }
}
#[repr(C, align(8))]
pub struct SInputToOrbitModifierParams {}
impl SInputToOrbitModifierParams {}
impl std::convert::AsRef<SInputToOrbitModifierParams> for SInputToOrbitModifierParams {
    fn as_ref(&self) -> &SInputToOrbitModifierParams {
        self
    }
}
impl std::convert::AsMut<SInputToOrbitModifierParams> for SInputToOrbitModifierParams {
    fn as_mut(&mut self) -> &mut SInputToOrbitModifierParams {
        self
    }
}
