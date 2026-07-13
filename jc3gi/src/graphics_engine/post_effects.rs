#![cfg_attr(any(), rustfmt::skip)]
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// Anti-aliasing resolve mode.
pub enum AAMode {
    AA_NONE = 0isize as _,
    AA_FXAA_COMPUTE = 1isize as _,
    AA_SMAA = 2isize as _,
    AA_SMAA_T2X = 3isize as _,
    AA_FXAA = 4isize as _,
}
fn _AAMode_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], AAMode>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// The anti-aliasing resolve. [`AAMode::AA_SMAA_T2X`] additionally reprojects against a previous-frame
/// history texture.
pub struct AntiAliasingEffect {
    _field_0: [u8; 768],
    pub m_Mode: crate::graphics_engine::post_effects::AAMode,
    _field_304: [u8; 4],
}
fn _AntiAliasingEffect_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x308], AntiAliasingEffect>([0u8; 0x308]);
    }
    unreachable!()
}
impl AntiAliasingEffect {
    pub const Apply_ADDRESS: usize = 0x1400BC9A0;
    /// `slot` is the in/out post-effect result-slot index.
    pub unsafe fn Apply(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
        mgr: *mut crate::graphics_engine::post_effects::PostEffectsManager,
        slot: *mut u32,
    ) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
                pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
                mgr: *mut crate::graphics_engine::post_effects::PostEffectsManager,
                slot: *mut u32,
            ) -> u64 = ::std::mem::transmute(Self::Apply_ADDRESS);
            f(self as *mut Self as _, ctx, pec, mgr, slot)
        }
    }
    pub const ApplySubsampleJitter_ADDRESS: usize = 0x1400C7700;
    /// Post-multiplies the sub-pixel clip-space jitter translation onto `proj`, only when the resolve
    /// mode is [`AAMode::AA_SMAA_T2X`]. The phase comes from the previous-frame counter parity.
    pub unsafe fn ApplySubsampleJitter(
        &self,
        proj: *mut crate::types::math::Matrix4,
        width: i32,
        height: i32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                proj: *mut crate::types::math::Matrix4,
                width: i32,
                height: i32,
            ) = ::std::mem::transmute(Self::ApplySubsampleJitter_ADDRESS);
            f(self as *const Self as _, proj, width, height)
        }
    }
    pub const CreateRenderTargetResources_ADDRESS: usize = 0x1400A5E30;
    /// Allocates the temporal history ping-pong textures and their render setups, sized `width` by
    /// `height`.
    pub unsafe fn CreateRenderTargetResources(
        &mut self,
        mgr: *const crate::graphics_engine::post_effects::PostEffectsManager,
        width: i32,
        height: i32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                mgr: *const crate::graphics_engine::post_effects::PostEffectsManager,
                width: i32,
                height: i32,
            ) = ::std::mem::transmute(Self::CreateRenderTargetResources_ADDRESS);
            f(self as *mut Self as _, mgr, width, height)
        }
    }
}
impl std::convert::AsRef<AntiAliasingEffect> for AntiAliasingEffect {
    fn as_ref(&self) -> &AntiAliasingEffect {
        self
    }
}
impl std::convert::AsMut<AntiAliasingEffect> for AntiAliasingEffect {
    fn as_mut(&mut self) -> &mut AntiAliasingEffect {
        self
    }
}
#[repr(C, align(8))]
/// The Gaussian blur, used on the non-bokeh path.
pub struct BlurEffect {}
impl BlurEffect {
    pub const Apply_ADDRESS: usize = 0x1400BCB10;
    pub unsafe fn Apply(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
        mgr: *mut crate::graphics_engine::post_effects::PostEffectsManager,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
                pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
                mgr: *mut crate::graphics_engine::post_effects::PostEffectsManager,
            ) -> bool = ::std::mem::transmute(Self::Apply_ADDRESS);
            f(self as *mut Self as _, ctx, pec, mgr)
        }
    }
}
impl std::convert::AsRef<BlurEffect> for BlurEffect {
    fn as_ref(&self) -> &BlurEffect {
        self
    }
}
impl std::convert::AsMut<BlurEffect> for BlurEffect {
    fn as_mut(&mut self) -> &mut BlurEffect {
        self
    }
}
#[repr(C, align(8))]
/// The bokeh blur, used when [`PostEffectsManager::IsBokehActive`]; runs after
/// [`DownScale2x2PackFocus`].
pub struct BlurEffectBokeh {}
impl BlurEffectBokeh {
    pub const Apply_ADDRESS: usize = 0x1400A7870;
    pub unsafe fn Apply(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
        mgr: *mut crate::graphics_engine::post_effects::PostEffectsManager,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
                pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
                mgr: *mut crate::graphics_engine::post_effects::PostEffectsManager,
            ) = ::std::mem::transmute(Self::Apply_ADDRESS);
            f(self as *mut Self as _, ctx, pec, mgr)
        }
    }
}
impl std::convert::AsRef<BlurEffectBokeh> for BlurEffectBokeh {
    fn as_ref(&self) -> &BlurEffectBokeh {
        self
    }
}
impl std::convert::AsMut<BlurEffectBokeh> for BlurEffectBokeh {
    fn as_mut(&mut self) -> &mut BlurEffectBokeh {
        self
    }
}
#[repr(C, align(8))]
pub struct DepthOfFieldEffect {}
impl DepthOfFieldEffect {
    pub const Apply_ADDRESS: usize = 0x1400C7890;
    pub unsafe fn Apply(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
        mgr: *mut crate::graphics_engine::post_effects::PostEffectsManager,
        input: u32,
    ) -> u32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
                pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
                mgr: *mut crate::graphics_engine::post_effects::PostEffectsManager,
                input: u32,
            ) -> u32 = ::std::mem::transmute(Self::Apply_ADDRESS);
            f(self as *mut Self as _, ctx, pec, mgr, input)
        }
    }
}
impl std::convert::AsRef<DepthOfFieldEffect> for DepthOfFieldEffect {
    fn as_ref(&self) -> &DepthOfFieldEffect {
        self
    }
}
impl std::convert::AsMut<DepthOfFieldEffect> for DepthOfFieldEffect {
    fn as_mut(&mut self) -> &mut DepthOfFieldEffect {
        self
    }
}
#[repr(C, align(8))]
/// The bokeh depth-of-field downscale prepass: a 2x2 pack plus focus.
pub struct DownScale2x2PackFocus {}
impl DownScale2x2PackFocus {
    pub const Apply_ADDRESS: usize = 0x1400C82E0;
    pub unsafe fn Apply(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
        mgr: *mut crate::graphics_engine::post_effects::PostEffectsManager,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
                pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
                mgr: *mut crate::graphics_engine::post_effects::PostEffectsManager,
            ) = ::std::mem::transmute(Self::Apply_ADDRESS);
            f(self as *mut Self as _, ctx, pec, mgr)
        }
    }
}
impl std::convert::AsRef<DownScale2x2PackFocus> for DownScale2x2PackFocus {
    fn as_ref(&self) -> &DownScale2x2PackFocus {
        self
    }
}
impl std::convert::AsMut<DownScale2x2PackFocus> for DownScale2x2PackFocus {
    fn as_mut(&mut self) -> &mut DownScale2x2PackFocus {
        self
    }
}
#[repr(C, align(8))]
/// The alpha-blended fade quad over the scene.
pub struct FadeEffect {}
impl FadeEffect {
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
impl std::convert::AsRef<FadeEffect> for FadeEffect {
    fn as_ref(&self) -> &FadeEffect {
        self
    }
}
impl std::convert::AsMut<FadeEffect> for FadeEffect {
    fn as_mut(&mut self) -> &mut FadeEffect {
        self
    }
}
#[repr(C, align(8))]
/// The bloom / glare generator. Writes its own scratch targets, composited later.
pub struct GlareEffect {}
impl GlareEffect {
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
impl std::convert::AsRef<GlareEffect> for GlareEffect {
    fn as_ref(&self) -> &GlareEffect {
        self
    }
}
impl std::convert::AsMut<GlareEffect> for GlareEffect {
    fn as_mut(&mut self) -> &mut GlareEffect {
        self
    }
}
#[repr(C, align(8))]
pub struct MotionBlurEffect {}
impl MotionBlurEffect {
    pub const Apply_ADDRESS: usize = 0x1400C8E20;
    pub unsafe fn Apply(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        pec: *mut crate::graphics_engine::post_effects::PostEffectContext,
        mgr: *mut crate::graphics_engine::post_effects::PostEffectsManager,
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
                mgr: *mut crate::graphics_engine::post_effects::PostEffectsManager,
                input: u32,
                blur: f32,
                flag0: bool,
                flag1: bool,
            ) -> u32 = ::std::mem::transmute(Self::Apply_ADDRESS);
            f(self as *mut Self as _, ctx, pec, mgr, input, blur, flag0, flag1)
        }
    }
}
impl std::convert::AsRef<MotionBlurEffect> for MotionBlurEffect {
    fn as_ref(&self) -> &MotionBlurEffect {
        self
    }
}
impl std::convert::AsMut<MotionBlurEffect> for MotionBlurEffect {
    fn as_mut(&mut self) -> &mut MotionBlurEffect {
        self
    }
}
#[repr(C, align(8))]
/// The red damage vignette. Returns the input slot index unchanged.
pub struct PlayerDamageEffect {}
impl PlayerDamageEffect {
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
impl std::convert::AsRef<PlayerDamageEffect> for PlayerDamageEffect {
    fn as_ref(&self) -> &PlayerDamageEffect {
        self
    }
}
impl std::convert::AsMut<PlayerDamageEffect> for PlayerDamageEffect {
    fn as_mut(&mut self) -> &mut PlayerDamageEffect {
        self
    }
}
#[repr(C, align(8))]
pub struct PostEffectContext {
    pub m_RenderContext: *mut crate::graphics_engine::post_effects::PostEffectRenderContext,
    _field_8: [u8; 148],
    /// The auto-exposure target numerator. [`ToneMappingEffect::Update`] sets the exposure target to
    /// this divided by the raw-brightness histogram mid-point.
    pub m_AutoExposureKey: f32,
}
fn _PostEffectContext_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xA0], PostEffectContext>([0u8; 0xA0]);
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
crate::__bitflags! {
    #[doc = " Per-frame render-context flags carried by a [`PostEffectRenderContext`]."]
    pub struct PostEffectRenderFlags : u8 { const m_MotionVectorReprojection = 1usize as
    _; }
}
fn _PostEffectRenderFlags_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1], PostEffectRenderFlags>([0u8; 0x1]);
    }
    unreachable!()
}
#[repr(C, align(8))]
pub struct PostEffectsManager {}
impl PostEffectsManager {
    pub const ApplyWorldFilters_ADDRESS: usize = 0x14014BFE0;
    /// Enqueues the world post-effect block, then steps the world fade accumulator
    /// ([`ApplyWorldFadeFilter`](PostEffectsManager::ApplyWorldFadeFilter)). `dt` flows only into
    /// that accumulator; the texture arguments are the scene inputs.
    pub unsafe fn ApplyWorldFilters(
        &mut self,
        dt: f32,
        setup: *mut crate::graphics_engine::graphics_engine::HRenderSetup_t,
        a4: *mut crate::graphics_engine::graphics_engine::HTexture_t,
        a5: *mut crate::graphics_engine::graphics_engine::HTexture_t,
        a6: *mut crate::graphics_engine::graphics_engine::HTexture_t,
        a7: *mut crate::graphics_engine::graphics_engine::HTexture_t,
        a8: *mut crate::graphics_engine::graphics_engine::HTexture_t,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                setup: *mut crate::graphics_engine::graphics_engine::HRenderSetup_t,
                a4: *mut crate::graphics_engine::graphics_engine::HTexture_t,
                a5: *mut crate::graphics_engine::graphics_engine::HTexture_t,
                a6: *mut crate::graphics_engine::graphics_engine::HTexture_t,
                a7: *mut crate::graphics_engine::graphics_engine::HTexture_t,
                a8: *mut crate::graphics_engine::graphics_engine::HTexture_t,
            ) = ::std::mem::transmute(Self::ApplyWorldFilters_ADDRESS);
            f(self as *mut Self as _, dt, setup, a4, a5, a6, a7, a8)
        }
    }
    pub const ApplyGlobalFilters_ADDRESS: usize = 0x14014C0C0;
    /// Enqueues the global post-effect block and advances its `dt`-driven accumulators: the screen
    /// fade alpha and the sun-direction / heat-haze accumulator.
    pub unsafe fn ApplyGlobalFilters(
        &mut self,
        dt: f32,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                dt: f32,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            ) = ::std::mem::transmute(Self::ApplyGlobalFilters_ADDRESS);
            f(self as *mut Self as _, dt, ctx)
        }
    }
    pub const ApplyWorldFadeFilter_ADDRESS: usize = 0x1400F9BD0;
    /// Steps the world fade accumulator.
    pub unsafe fn ApplyWorldFadeFilter(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::ApplyWorldFadeFilter_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const IsBokehActive_ADDRESS: usize = 0x1400A0270;
    /// Whether the bokeh depth-of-field path is active, selecting the downscale plus bokeh blur over
    /// the plain [`BlurEffect`].
    pub unsafe fn IsBokehActive(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsBokehActive_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const IsMotionBlurActive_ADDRESS: usize = 0x1400FA3E0;
    /// Whether motion blur is active.
    pub unsafe fn IsMotionBlurActive(&self) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> bool = ::std::mem::transmute(
                Self::IsMotionBlurActive_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const ApplySubsampleJitter_ADDRESS: usize = 0x1400FA050;
    /// Post-multiplies the temporal sub-pixel jitter onto `proj`. Effective only when the resolve mode
    /// is [`AAMode::AA_SMAA_T2X`].
    pub unsafe fn ApplySubsampleJitter(
        &self,
        proj: *mut crate::types::math::Matrix4,
        width: i32,
        height: i32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                proj: *mut crate::types::math::Matrix4,
                width: i32,
                height: i32,
            ) = ::std::mem::transmute(Self::ApplySubsampleJitter_ADDRESS);
            f(self as *const Self as _, proj, width, height)
        }
    }
}
impl std::convert::AsRef<PostEffectsManager> for PostEffectsManager {
    fn as_ref(&self) -> &PostEffectsManager {
        self
    }
}
impl std::convert::AsMut<PostEffectsManager> for PostEffectsManager {
    fn as_mut(&mut self) -> &mut PostEffectsManager {
        self
    }
}
#[repr(C, align(8))]
/// The render block for the post-effects pass. Its [`Draw`](RenderBlockPostEffects::Draw) runs the
/// HDR post chain in order: histogram generation, sun-halo pre-apply, blur (bokeh or plain), glare,
/// depth of field, motion blur, the HDR-to-LDR tonemap, the player-damage vignette, anti-aliasing,
/// sun halo, and the final fade.
///
/// It threads a single result-texture slot index through the slot-returning effects
/// ([`DepthOfFieldEffect`], [`MotionBlurEffect`], [`PlayerDamageEffect`], [`AntiAliasingEffect`]),
/// hopping between the three fullscreen temp textures.
pub struct RenderBlockPostEffects {}
impl RenderBlockPostEffects {
    pub const Draw_ADDRESS: usize = 0x14016A260;
    pub unsafe fn Draw(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
        info: *const crate::graphics_engine::render_pass::RBIInfo,
    ) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
                info: *const crate::graphics_engine::render_pass::RBIInfo,
            ) -> u64 = ::std::mem::transmute(Self::Draw_ADDRESS);
            f(self as *mut Self as _, ctx, info)
        }
    }
}
impl std::convert::AsRef<RenderBlockPostEffects> for RenderBlockPostEffects {
    fn as_ref(&self) -> &RenderBlockPostEffects {
        self
    }
}
impl std::convert::AsMut<RenderBlockPostEffects> for RenderBlockPostEffects {
    fn as_mut(&mut self) -> &mut RenderBlockPostEffects {
        self
    }
}
#[repr(C, align(8))]
/// The sun halo. [`PreApply`](SunHaloEffect::PreApply) prepares it and sets the ready flag;
/// [`Apply`](SunHaloEffect::Apply) composites it.
pub struct SunHaloEffect {
    _field_0: [u8; 276],
    /// The ready flag: [`PreApply`](SunHaloEffect::PreApply) sets it when the halo is prepared, and
    /// [`Apply`](SunHaloEffect::Apply) early-outs when it is clear.
    pub m_Ready: bool,
    _field_115: [u8; 3],
}
fn _SunHaloEffect_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x118], SunHaloEffect>([0u8; 0x118]);
    }
    unreachable!()
}
impl SunHaloEffect {
    pub const PreApply_ADDRESS: usize = 0x140118450;
    pub unsafe fn PreApply(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        a3: *mut ::std::ffi::c_void,
        mgr: *mut crate::graphics_engine::post_effects::PostEffectsManager,
    ) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
                a3: *mut ::std::ffi::c_void,
                mgr: *mut crate::graphics_engine::post_effects::PostEffectsManager,
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
impl std::convert::AsRef<SunHaloEffect> for SunHaloEffect {
    fn as_ref(&self) -> &SunHaloEffect {
        self
    }
}
impl std::convert::AsMut<SunHaloEffect> for SunHaloEffect {
    fn as_mut(&mut self) -> &mut SunHaloEffect {
        self
    }
}
