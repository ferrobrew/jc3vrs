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
    /// Ping-pong selector for the exposure-weighted histogram metering, flipped each frame by Update.
    pub m_HistogramPingPong: u32,
    _field_2a4: [u8; 4],
    /// A second histogram, processed by Update's second CalculateMidAndBrightPointForHistogram call.
    /// Its m_HistogramMidPoint is the divisor of the auto-exposure target (target = key / midpoint),
    /// so this -- not m_Histogram -- is what the converged m_CurrentExposure actually tracks.
    pub m_Histogram2: crate::graphics_engine::tone_mapping::SHistogramGeneration,
    _field_540: [u8; 56],
    /// The active histogram bucket count.
    pub m_NumBuckets: u32,
    _field_57c: [u8; 1652],
    /// The current clamped/smoothed auto-exposure multiplier. Written by Update once per frame and
    /// read back into the next frame's GenerateHistogramForFinalScene metering (the feedback loop).
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
    /// indices to a6 / a7 (out-params); returns a7. Meters luminance through a
    /// "LuminanceToDepthWithExposure" shader fed the previous frame's m_CurrentExposure as a
    /// constant, so metering is exposure-weighted -- the histogram feeds back into the exposure that
    /// weights the next frame's metering (via CalculateMidAndBrightPointForHistogram and Update).
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
    /// Meters the second, NON-exposure-weighted scene-luminance histogram (m_Histogram2) -- it calls
    /// PopulateHistogram with a fixed exposure of 1.0, so it measures raw scene brightness, which is
    /// the value Update divides the auto-exposure target by. Runs once per dispatch in the post chain,
    /// like GenerateHistogramForFinalScene (which meters the exposure-weighted m_Histogram).
    pub unsafe fn DrawHistogramWindow(
        &self,
        ctx: *mut ::std::ffi::c_void,
        pec: *mut ::std::ffi::c_void,
        mgr: *mut ::std::ffi::c_void,
        index: u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                ctx: *mut ::std::ffi::c_void,
                pec: *mut ::std::ffi::c_void,
                mgr: *mut ::std::ffi::c_void,
                index: u32,
            ) = ::std::mem::transmute(Self::DrawHistogramWindow_ADDRESS);
            f(self as *const Self as _, ctx, pec, mgr, index)
        }
    }
    pub const Update_ADDRESS: usize = 0x140119560;
    /// The per-frame eye-adaptation step: runs CalculateMidAndBrightPointForHistogram over both
    /// histograms, then writes the new m_CurrentExposure (target = ctx.m_AutoExposureKey /
    /// m_Histogram2 midpoint, clamped, then adapted). Runs once per real frame from
    /// CPostEffectsManager::UpdateRender.
    pub unsafe fn Update(
        &mut self,
        manager: *mut ::std::ffi::c_void,
        ctx: *mut crate::graphics_engine::post_effects::PostEffectContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                manager: *mut ::std::ffi::c_void,
                ctx: *mut crate::graphics_engine::post_effects::PostEffectContext,
            ) = ::std::mem::transmute(Self::Update_ADDRESS);
            f(self as *mut Self as _, manager, ctx)
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
