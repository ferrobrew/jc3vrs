#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// Per-frame brightness histogram used for eye adaptation. Embedded in ToneMappingEffect at +8; its
/// occlusion-query bucket counts are what the exposure read-back consumes.
pub struct SHistogramGeneration {
    _field_0: [u8; 400],
    /// Per-bucket pixel counts: one occlusion query per bucket counting samples whose luminance is
    /// at or above the bucket's threshold. Only the first m_NumBuckets entries are live.
    pub m_NumPixelsInBuckets: [u32; 64],
    /// Bright-point computed from the buckets; drives the exposure target.
    pub m_HistogramBrightPoint: f32,
    /// Mid-point computed from the buckets.
    pub m_HistogramMidPoint: f32,
}
fn _SHistogramGeneration_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x298], SHistogramGeneration>([0u8; 0x298]);
    }
    unreachable!()
}
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
/// N-frame exposure smoother (a ring-buffer average; advances once per render, with no dt term).
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
#[repr(C, align(8))]
/// HDR tone-mapping / auto-exposure (eye adaptation).
pub struct ToneMappingEffect {
    _field_0: [u8; 8],
    /// The exposure histogram: CalculateMidAndBrightPointForHistogram fills it, Update reads it.
    pub m_Histogram: crate::graphics_engine::tone_mapping::SHistogramGeneration,
    _field_2a0: [u8; 728],
    /// The active histogram bucket count.
    pub m_NumBuckets: u32,
    _field_57c: [u8; 1652],
    /// The current clamped/smoothed auto-exposure multiplier.
    pub m_CurrentExposure: f32,
    _field_bf4: [u8; 4],
}
fn _ToneMappingEffect_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xBF8], ToneMappingEffect>([0u8; 0xBF8]);
    }
    unreachable!()
}
impl ToneMappingEffect {
    pub const GenerateHistogramForFinalScene_ADDRESS: usize = 0x140119440;
    /// Builds the auto-exposure histogram for the final scene and writes the current histogram slot
    /// indices to a6 / a7 (out-params); returns a7.
    pub unsafe fn GenerateHistogramForFinalScene(
        &mut self,
        ctx: *mut ::std::ffi::c_void,
        a3: *mut ::std::ffi::c_void,
        a4: *mut ::std::ffi::c_void,
        a5: i32,
        a6: *mut u32,
        a7: *mut u32,
    ) -> *mut u32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut ::std::ffi::c_void,
                a3: *mut ::std::ffi::c_void,
                a4: *mut ::std::ffi::c_void,
                a5: i32,
                a6: *mut u32,
                a7: *mut u32,
            ) -> *mut u32 = ::std::mem::transmute(
                Self::GenerateHistogramForFinalScene_ADDRESS,
            );
            f(self as *mut Self as _, ctx, a3, a4, a5, a6, a7)
        }
    }
    pub const DrawHistogramWindow_ADDRESS: usize = 0x1401198F0;
    /// The HDR->LDR tonemap composite: applies the current exposure (from the histogram) to convert
    /// the HDR MainColor into the LDR target.
    pub unsafe fn DrawHistogramWindow(
        &self,
        ctx: *mut ::std::ffi::c_void,
        pec: *mut ::std::ffi::c_void,
        mgr: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                ctx: *mut ::std::ffi::c_void,
                pec: *mut ::std::ffi::c_void,
                mgr: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::DrawHistogramWindow_ADDRESS);
            f(self as *const Self as _, ctx, pec, mgr)
        }
    }
}
impl std::convert::AsRef<ToneMappingEffect> for ToneMappingEffect {
    fn as_ref(&self) -> &ToneMappingEffect {
        self
    }
}
impl std::convert::AsMut<ToneMappingEffect> for ToneMappingEffect {
    fn as_mut(&mut self) -> &mut ToneMappingEffect {
        self
    }
}
pub const CalculateMidAndBrightPointForHistogram_ADDRESS: usize = 0x1400F8BF0;
/// Computes the histogram mid / bright points (the per-frame eye-adaptation lerp). Free function;
/// `hist` is the target SHistogramGeneration.
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
