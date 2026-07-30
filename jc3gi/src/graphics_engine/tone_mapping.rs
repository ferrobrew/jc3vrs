#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A per-frame brightness histogram used for eye adaptation. Its occlusion-query bucket counts are
/// what the exposure read-back consumes.
pub struct HistogramGeneration {
    _field_0: [u8; 400],
    /// The per-bucket pixel counts: one occlusion query per bucket, counting samples whose luminance
    /// is at or above the bucket's threshold. Only the first `m_NumBuckets` entries are live.
    pub m_NumPixelsInBuckets: [u32; 64],
    /// The bright-point computed from the buckets, driving the exposure target.
    pub m_HistogramBrightPoint: f32,
    /// The mid-point computed from the buckets.
    pub m_HistogramMidPoint: f32,
}
fn _HistogramGeneration_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x298], HistogramGeneration>([0u8; 0x298]);
    }
    unreachable!()
}
impl HistogramGeneration {}
impl std::convert::AsRef<HistogramGeneration> for HistogramGeneration {
    fn as_ref(&self) -> &HistogramGeneration {
        self
    }
}
impl std::convert::AsMut<HistogramGeneration> for HistogramGeneration {
    fn as_mut(&mut self) -> &mut HistogramGeneration {
        self
    }
}
#[repr(C, align(8))]
/// An N-frame exposure smoother: a ring-buffer average that advances once per render, with no `dt`
/// term.
pub struct SmoothedExposure {}
impl SmoothedExposure {
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
impl std::convert::AsRef<SmoothedExposure> for SmoothedExposure {
    fn as_ref(&self) -> &SmoothedExposure {
        self
    }
}
impl std::convert::AsMut<SmoothedExposure> for SmoothedExposure {
    fn as_mut(&mut self) -> &mut SmoothedExposure {
        self
    }
}
#[repr(C, align(8))]
/// HDR tone-mapping and auto-exposure (eye adaptation).
pub struct ToneMappingEffect {
    _field_0: [u8; 8],
    /// The exposure-weighted histogram, filled by
    /// [`CalculateMidAndBrightPointForHistogram`](crate::graphics_engine::tone_mapping::CalculateMidAndBrightPointForHistogram) and read by [`Update`](crate::graphics_engine::tone_mapping::ToneMappingEffect::Update).
    pub m_Histogram: crate::graphics_engine::tone_mapping::HistogramGeneration,
    /// The ping-pong selector for the exposure-weighted histogram metering, flipped each frame by
    /// [`Update`](crate::graphics_engine::tone_mapping::ToneMappingEffect::Update).
    pub m_HistogramPingPong: u32,
    _field_2a4: [u8; 4],
    /// The second histogram, metering raw scene brightness. Its `m_HistogramMidPoint` is the divisor
    /// of the auto-exposure target (`target = key / midpoint`), so this -- not `m_Histogram` -- is
    /// what the converged [`m_ExposureBrightPoint`](crate::graphics_engine::tone_mapping::ToneMappingEffect::m_ExposureBrightPoint) actually tracks.
    pub m_Histogram2: crate::graphics_engine::tone_mapping::HistogramGeneration,
    _field_540: [u8; 56],
    /// The active histogram bucket count.
    pub m_NumBuckets: u32,
    _field_57c: [u8; 1652],
    /// The applied auto-exposure multiplier: the clamped, smoothed (EMA) value
    /// [`Update`](crate::graphics_engine::tone_mapping::ToneMappingEffect::Update) writes once per frame. [`Apply`](crate::graphics_engine::tone_mapping::ToneMappingEffect::Apply) publishes it
    /// into the frame's [`PostEffectContext::m_Exposure`](crate::graphics_engine::post_effects::PostEffectContext) --
    /// the value the HDR-to-LDR composite multiplies the scene by -- and feeds it back into the
    /// exposure-weighted histogram metering, closing the adaptation loop.
    pub m_ExposureBrightPoint: f32,
    /// The EMA'd bright point of the exposure-weighted histogram, published alongside
    /// [`m_ExposureBrightPoint`](crate::graphics_engine::tone_mapping::ToneMappingEffect::m_ExposureBrightPoint) by [`Apply`](crate::graphics_engine::tone_mapping::ToneMappingEffect::Apply).
    pub m_HistogramBrightPointWithExposure: f32,
}
fn _ToneMappingEffect_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xBF8], ToneMappingEffect>([0u8; 0xBF8]);
    }
    unreachable!()
}
impl ToneMappingEffect {
    pub const Apply_ADDRESS: usize = 0x140119440;
    /// The per-dispatch exposure publish and exposure-weighted meter, run at the start of the
    /// post-effects block: publishes [`m_ExposureBrightPoint`](crate::graphics_engine::tone_mapping::ToneMappingEffect::m_ExposureBrightPoint) and
    /// [`m_HistogramBrightPointWithExposure`](crate::graphics_engine::tone_mapping::ToneMappingEffect::m_HistogramBrightPointWithExposure) through
    /// `out_exposure` / `out_bright_point` (the caller passes the frame context's `m_Exposure` /
    /// `m_HistogramBrightPointAfterExposure`), and meters luminance through a shader fed the
    /// published exposure, so that histogram's metering is exposure-weighted.
    pub unsafe fn Apply(
        &mut self,
        ctx: *mut ::std::ffi::c_void,
        mgr: *mut ::std::ffi::c_void,
        pec: *mut ::std::ffi::c_void,
        dt: f32,
        out_exposure: *mut f32,
        out_bright_point: *mut f32,
    ) -> *mut f32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut ::std::ffi::c_void,
                mgr: *mut ::std::ffi::c_void,
                pec: *mut ::std::ffi::c_void,
                dt: f32,
                out_exposure: *mut f32,
                out_bright_point: *mut f32,
            ) -> *mut f32 = ::std::mem::transmute(Self::Apply_ADDRESS);
            f(self as *mut Self as _, ctx, mgr, pec, dt, out_exposure, out_bright_point)
        }
    }
    pub const GenerateHistogramForFinalScene_ADDRESS: usize = 0x1401198F0;
    /// Meters the non-exposure-weighted final-scene luminance histogram (`m_Histogram2`) at a fixed
    /// exposure of `1.0`, so it measures raw scene brightness -- the value
    /// [`Update`](crate::graphics_engine::tone_mapping::ToneMappingEffect::Update) divides the auto-exposure target by. Runs once per
    /// dispatch in the post chain, after the effect chain.
    pub unsafe fn GenerateHistogramForFinalScene(
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
            ) = ::std::mem::transmute(Self::GenerateHistogramForFinalScene_ADDRESS);
            f(self as *const Self as _, ctx, pec, mgr, index)
        }
    }
    pub const Update_ADDRESS: usize = 0x140119560;
    /// The per-frame eye-adaptation step: runs [`CalculateMidAndBrightPointForHistogram`](crate::graphics_engine::tone_mapping::CalculateMidAndBrightPointForHistogram) over both
    /// histograms, then adapts [`m_ExposureBrightPoint`](crate::graphics_engine::tone_mapping::ToneMappingEffect::m_ExposureBrightPoint) (the target is
    /// `m_AutoExposureKey` over the `m_Histogram2` mid-point, clamped, then EMA'd). Runs once per
    /// real frame.
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
/// Computes the histogram mid and bright points, the per-frame eye-adaptation lerp.
unsafe fn CalculateMidAndBrightPointForHistogram(
    ctx: *mut ::std::ffi::c_void,
    arg1: f32,
    arg2: i32,
    arg3: f32,
    hist: *mut crate::graphics_engine::tone_mapping::HistogramGeneration,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut ::std::ffi::c_void,
            arg1: f32,
            arg2: i32,
            arg3: f32,
            hist: *mut crate::graphics_engine::tone_mapping::HistogramGeneration,
        ) = ::std::mem::transmute(CalculateMidAndBrightPointForHistogram_ADDRESS);
        f(ctx, arg1, arg2, arg3, hist)
    }
}
