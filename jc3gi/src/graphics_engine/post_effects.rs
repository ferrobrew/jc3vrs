#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// Anti-aliasing resolve. `m_Mode`: 1/4 = FXAA, 2 = SMAA 1x, 3 = SMAA T2X. Mode 3 adds a temporal
/// reprojection against a previous-frame history texture.
pub struct CAntiAliasingEffect {
    _field_0: [u8; 768],
    pub m_Mode: i32,
    _field_304: [u8; 4],
}
fn _CAntiAliasingEffect_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x308], CAntiAliasingEffect>([0u8; 0x308]);
    }
    unreachable!()
}
impl CAntiAliasingEffect {
    pub const Apply_ADDRESS: usize = 0x1400BC9A0;
    /// `slot` is the in/out post-effect result-slot index.
    pub unsafe fn Apply(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
        mgr: *mut crate::graphics_engine::post_effects::CPostEffectsManager,
        slot: *mut u32,
    ) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
                pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
                mgr: *mut crate::graphics_engine::post_effects::CPostEffectsManager,
                slot: *mut u32,
            ) -> u64 = ::std::mem::transmute(Self::Apply_ADDRESS);
            f(self as *mut Self as _, ctx, pec, mgr, slot)
        }
    }
}
impl std::convert::AsRef<CAntiAliasingEffect> for CAntiAliasingEffect {
    fn as_ref(&self) -> &CAntiAliasingEffect {
        self
    }
}
impl std::convert::AsMut<CAntiAliasingEffect> for CAntiAliasingEffect {
    fn as_mut(&mut self) -> &mut CAntiAliasingEffect {
        self
    }
}
#[repr(C, align(8))]
pub struct CDepthOfFieldEffect {}
impl CDepthOfFieldEffect {
    pub const Apply_ADDRESS: usize = 0x1400C7890;
    pub unsafe fn Apply(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
        mgr: *mut crate::graphics_engine::post_effects::CPostEffectsManager,
        input: u32,
    ) -> u32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
                pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
                mgr: *mut crate::graphics_engine::post_effects::CPostEffectsManager,
                input: u32,
            ) -> u32 = ::std::mem::transmute(Self::Apply_ADDRESS);
            f(self as *mut Self as _, ctx, pec, mgr, input)
        }
    }
}
impl std::convert::AsRef<CDepthOfFieldEffect> for CDepthOfFieldEffect {
    fn as_ref(&self) -> &CDepthOfFieldEffect {
        self
    }
}
impl std::convert::AsMut<CDepthOfFieldEffect> for CDepthOfFieldEffect {
    fn as_mut(&mut self) -> &mut CDepthOfFieldEffect {
        self
    }
}
#[repr(C, align(8))]
/// Alpha-blended fade quad over the scene.
pub struct CFadeEffect {}
impl CFadeEffect {
    pub const Apply_ADDRESS: usize = 0x1400A9570;
    pub unsafe fn Apply(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        a3: *mut ::std::ffi::c_void,
    ) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
                a3: *mut ::std::ffi::c_void,
            ) -> u64 = ::std::mem::transmute(Self::Apply_ADDRESS);
            f(self as *mut Self as _, ctx, a3)
        }
    }
}
impl std::convert::AsRef<CFadeEffect> for CFadeEffect {
    fn as_ref(&self) -> &CFadeEffect {
        self
    }
}
impl std::convert::AsMut<CFadeEffect> for CFadeEffect {
    fn as_mut(&mut self) -> &mut CFadeEffect {
        self
    }
}
#[repr(C, align(8))]
/// Bloom / glare generator (writes its own scratch targets, composited later).
pub struct CGlareEffect {}
impl CGlareEffect {
    pub const Apply_ADDRESS: usize = 0x1400AA510;
    pub unsafe fn Apply(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
        a4: *mut ::std::ffi::c_void,
        a5: *mut ::std::ffi::c_void,
    ) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
                pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
                a4: *mut ::std::ffi::c_void,
                a5: *mut ::std::ffi::c_void,
            ) -> u64 = ::std::mem::transmute(Self::Apply_ADDRESS);
            f(self as *mut Self as _, ctx, pec, a4, a5)
        }
    }
}
impl std::convert::AsRef<CGlareEffect> for CGlareEffect {
    fn as_ref(&self) -> &CGlareEffect {
        self
    }
}
impl std::convert::AsMut<CGlareEffect> for CGlareEffect {
    fn as_mut(&mut self) -> &mut CGlareEffect {
        self
    }
}
#[repr(C, align(8))]
pub struct CMotionBlurEffect {}
impl CMotionBlurEffect {
    pub const Apply_ADDRESS: usize = 0x1400C8E20;
    pub unsafe fn Apply(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
        mgr: *mut crate::graphics_engine::post_effects::CPostEffectsManager,
        input: u32,
        blur: f32,
        flag0: bool,
        flag1: bool,
    ) -> u32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
                pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
                mgr: *mut crate::graphics_engine::post_effects::CPostEffectsManager,
                input: u32,
                blur: f32,
                flag0: bool,
                flag1: bool,
            ) -> u32 = ::std::mem::transmute(Self::Apply_ADDRESS);
            f(self as *mut Self as _, ctx, pec, mgr, input, blur, flag0, flag1)
        }
    }
}
impl std::convert::AsRef<CMotionBlurEffect> for CMotionBlurEffect {
    fn as_ref(&self) -> &CMotionBlurEffect {
        self
    }
}
impl std::convert::AsMut<CMotionBlurEffect> for CMotionBlurEffect {
    fn as_mut(&mut self) -> &mut CMotionBlurEffect {
        self
    }
}
#[repr(C, align(8))]
/// Red damage vignette. Slot-passthrough (returns the input slot index).
pub struct CPlayerDamageEffect {}
impl CPlayerDamageEffect {
    pub const Apply_ADDRESS: usize = 0x1400F76E0;
    pub unsafe fn Apply(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
        a4: *mut ::std::ffi::c_void,
        input: u32,
    ) -> u32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
                pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
                a4: *mut ::std::ffi::c_void,
                input: u32,
            ) -> u32 = ::std::mem::transmute(Self::Apply_ADDRESS);
            f(self as *mut Self as _, ctx, pec, a4, input)
        }
    }
}
impl std::convert::AsRef<CPlayerDamageEffect> for CPlayerDamageEffect {
    fn as_ref(&self) -> &CPlayerDamageEffect {
        self
    }
}
impl std::convert::AsMut<CPlayerDamageEffect> for CPlayerDamageEffect {
    fn as_mut(&mut self) -> &mut CPlayerDamageEffect {
        self
    }
}
#[repr(C, align(8))]
pub struct CPostEffectsManager {}
impl CPostEffectsManager {}
impl std::convert::AsRef<CPostEffectsManager> for CPostEffectsManager {
    fn as_ref(&self) -> &CPostEffectsManager {
        self
    }
}
impl std::convert::AsMut<CPostEffectsManager> for CPostEffectsManager {
    fn as_mut(&mut self) -> &mut CPostEffectsManager {
        self
    }
}
#[repr(C, align(8))]
/// Sun halo. PreApply prepares and sets the ready flag (byte at +0x114); Apply composites it.
pub struct CSunHaloEffect {}
impl CSunHaloEffect {
    pub const PreApply_ADDRESS: usize = 0x140118450;
    pub unsafe fn PreApply(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        a3: *mut ::std::ffi::c_void,
        mgr: *mut crate::graphics_engine::post_effects::CPostEffectsManager,
    ) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
                a3: *mut ::std::ffi::c_void,
                mgr: *mut crate::graphics_engine::post_effects::CPostEffectsManager,
            ) -> u64 = ::std::mem::transmute(Self::PreApply_ADDRESS);
            f(self as *mut Self as _, ctx, a3, mgr)
        }
    }
    pub const Apply_ADDRESS: usize = 0x1400F8030;
    pub unsafe fn Apply(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    ) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            ) -> u64 = ::std::mem::transmute(Self::Apply_ADDRESS);
            f(self as *mut Self as _, ctx)
        }
    }
}
impl std::convert::AsRef<CSunHaloEffect> for CSunHaloEffect {
    fn as_ref(&self) -> &CSunHaloEffect {
        self
    }
}
impl std::convert::AsMut<CSunHaloEffect> for CSunHaloEffect {
    fn as_mut(&mut self) -> &mut CSunHaloEffect {
        self
    }
}
#[repr(C, align(8))]
pub struct PostEffectContext {
    pub m_RenderContext: *mut crate::graphics_engine::post_effects::PostEffectRenderContext,
}
fn _PostEffectContext_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], PostEffectContext>([0u8; 0x8]);
    }
    unreachable!()
}
impl PostEffectContext {}
impl std::convert::AsRef<PostEffectContext> for PostEffectContext {
    fn as_ref(&self) -> &PostEffectContext {
        self
    }
}
impl std::convert::AsMut<PostEffectContext> for PostEffectContext {
    fn as_mut(&mut self) -> &mut PostEffectContext {
        self
    }
}
#[repr(C, align(8))]
pub struct PostEffectRenderContext {
    _field_0: [u8; 900],
    pub m_Flags: crate::graphics_engine::post_effects::PostEffectRenderFlags,
    _field_385: [u8; 3],
}
fn _PostEffectRenderContext_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x388], PostEffectRenderContext>([0u8; 0x388]);
    }
    unreachable!()
}
impl PostEffectRenderContext {}
impl std::convert::AsRef<PostEffectRenderContext> for PostEffectRenderContext {
    fn as_ref(&self) -> &PostEffectRenderContext {
        self
    }
}
impl std::convert::AsMut<PostEffectRenderContext> for PostEffectRenderContext {
    fn as_mut(&mut self) -> &mut PostEffectRenderContext {
        self
    }
}
bitflags::bitflags! {
    #[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)] #[doc =
    " Per-frame render-context flags."] pub struct PostEffectRenderFlags : u8 { const
    m_MotionVectorReprojection = 1usize as _; }
}
fn _PostEffectRenderFlags_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1], PostEffectRenderFlags>([0u8; 0x1]);
    }
    unreachable!()
}
