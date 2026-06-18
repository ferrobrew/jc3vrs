#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
pub struct CToneMappingEffect {}
impl CToneMappingEffect {}
impl std::convert::AsRef<CToneMappingEffect> for CToneMappingEffect {
    fn as_ref(&self) -> &CToneMappingEffect {
        self
    }
}
impl std::convert::AsMut<CToneMappingEffect> for CToneMappingEffect {
    fn as_mut(&mut self) -> &mut CToneMappingEffect {
        self
    }
}
#[repr(C, align(8))]
pub struct SHistogramGeneration {}
impl SHistogramGeneration {}
impl std::convert::AsRef<SHistogramGeneration> for SHistogramGeneration {
    fn as_ref(&self) -> &SHistogramGeneration {
        self
    }
}
impl std::convert::AsMut<SHistogramGeneration> for SHistogramGeneration {
    fn as_mut(&mut self) -> &mut SHistogramGeneration {
        self
    }
}
#[repr(C, align(8))]
pub struct SSmoothedExposure {}
impl SSmoothedExposure {
    pub const Update_ADDRESS: usize = 0x1400F8200;
    pub unsafe fn Update(&mut self, exposure: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, exposure: f32) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _, exposure)
        }
    }
}
impl std::convert::AsRef<SSmoothedExposure> for SSmoothedExposure {
    fn as_ref(&self) -> &SSmoothedExposure {
        self
    }
}
impl std::convert::AsMut<SSmoothedExposure> for SSmoothedExposure {
    fn as_mut(&mut self) -> &mut SSmoothedExposure {
        self
    }
}
pub const CalculateMidAndBrightPointForHistogram_ADDRESS: usize = 0x1400F8BF0;
unsafe fn CalculateMidAndBrightPointForHistogram(
    ctx: *mut ::std::ffi::c_void,
    arg1: f32,
    arg2: i32,
    arg3: f32,
    hist: *mut crate::graphics_engine::tone_mapping::SHistogramGeneration,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut ::std::ffi::c_void,
            arg1: f32,
            arg2: i32,
            arg3: f32,
            hist: *mut crate::graphics_engine::tone_mapping::SHistogramGeneration,
        ) = ::std::mem::transmute(CalculateMidAndBrightPointForHistogram_ADDRESS);
        f(ctx, arg1, arg2, arg3, hist)
    }
}
