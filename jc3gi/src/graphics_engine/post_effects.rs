#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
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
    pub m_Flags: u8,
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
