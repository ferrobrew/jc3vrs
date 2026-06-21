#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(1))]
pub struct RenderEngine {
    _field_0: [u8; 8736],
}
fn _RenderEngine_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x2220], RenderEngine>([0u8; 0x2220]);
    }
    unreachable!()
}
impl RenderEngine {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417799192usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl RenderEngine {
    pub const PostDraw_ADDRESS: usize = 0x1401C2350;
    /// Late render-pass step (finalizes / copies render targets under the context mutex).
    pub unsafe fn PostDraw(
        &mut self,
        context: *const crate::graphics_engine::graphics_engine::HContext_t,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                context: *const crate::graphics_engine::graphics_engine::HContext_t,
            ) = ::std::mem::transmute(Self::PostDraw_ADDRESS);
            f(self as *mut Self as _, context)
        }
    }
    pub const DrawRenderPassRange_ADDRESS: usize = 0x140186600;
    /// Draws every render block in the half-open pass-index range [first, last): for each pass it
    /// walks the fixed RenderPass* array at this + 32*pass + 128 and vtable-dispatches each block.
    /// GBuffer is 0x2F..0x55, the lighting/scene block 0x56..0x96, post-effects 0x96..0x97.
    pub unsafe fn DrawRenderPassRange(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        setup: *mut crate::graphics_engine::graphics_engine::HRenderSetup_t,
        first: crate::graphics_engine::render_engine::RenderPassId,
        last: crate::graphics_engine::render_engine::RenderPassId,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
                setup: *mut crate::graphics_engine::graphics_engine::HRenderSetup_t,
                first: crate::graphics_engine::render_engine::RenderPassId,
                last: crate::graphics_engine::render_engine::RenderPassId,
            ) = ::std::mem::transmute(Self::DrawRenderPassRange_ADDRESS);
            f(self as *mut Self as _, ctx, setup, first, last)
        }
    }
    pub const DrawGBuffer_ADDRESS: usize = 0x140186810;
    /// GBuffer fill: binds two global textures, then DrawRenderPassRange(0x2F, 0x55) (depth /
    /// velocity prefix, static/dynamic models, decals).
    pub unsafe fn DrawGBuffer(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        a3: i64,
        a4: *mut crate::graphics_engine::graphics_engine::HTexture_t,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
                a3: i64,
                a4: *mut crate::graphics_engine::graphics_engine::HTexture_t,
            ) = ::std::mem::transmute(Self::DrawGBuffer_ADDRESS);
            f(self as *mut Self as _, ctx, a3, a4)
        }
    }
    pub const Draw_ADDRESS: usize = 0x1401868A0;
    /// Lighting / reflections / opaque / environment / water / transparency:
    /// DrawRenderPassRange(0x56, 0x96), then clears the global texture samplers.
    pub unsafe fn Draw(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    ) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            ) -> u64 = ::std::mem::transmute(Self::Draw_ADDRESS);
            f(self as *mut Self as _, ctx)
        }
    }
    pub const DrawPosteffects_ADDRESS: usize = 0x140186910;
    /// Post-effects pass: DrawRenderPassRange(0x96, 0x97) (the RP_POSTEFFECTS pass, whose block is
    /// RenderBlockPostEffects::Draw).
    pub unsafe fn DrawPosteffects(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        setup: *mut crate::graphics_engine::graphics_engine::HRenderSetup_t,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
                setup: *mut crate::graphics_engine::graphics_engine::HRenderSetup_t,
            ) = ::std::mem::transmute(Self::DrawPosteffects_ADDRESS);
            f(self as *mut Self as _, ctx, setup)
        }
    }
    pub const SetGlobalShaderConstants_ADDRESS: usize = 0x140185740;
    /// Uploads the global per-view constant buffer for the frame: lighting, fog, wetness, and the
    /// render camera's full (translation-bearing) m_ViewProjectionF and world position. This block
    /// drives screen-space / non-geometry work, not opaque-geometry vertex placement.
    pub unsafe fn SetGlobalShaderConstants(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
            ) = ::std::mem::transmute(Self::SetGlobalShaderConstants_ADDRESS);
            f(self as *mut Self as _, ctx)
        }
    }
    pub const ApplyJitterTransform_ADDRESS: usize = 0x140173AA0;
    /// Per-frame TAA jitter: forwards to PostEffectsManager::ApplySubsampleJitter, which
    /// post-multiplies a sub-pixel clip-space translation onto `proj` only when AA mode == 3.
    pub unsafe fn ApplyJitterTransform(
        &mut self,
        proj: *mut crate::types::math::Matrix4,
        width: i32,
        height: i32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                proj: *mut crate::types::math::Matrix4,
                width: i32,
                height: i32,
            ) = ::std::mem::transmute(Self::ApplyJitterTransform_ADDRESS);
            f(self as *mut Self as _, proj, width, height)
        }
    }
    pub const EraseAllDeletedRenderBlocks_ADDRESS: usize = 0x1401A4ED0;
    /// Drains a separate deferred deletion list of render blocks (under its own critical section).
    /// Does not touch the per-pass draw lists.
    pub unsafe fn EraseAllDeletedRenderBlocks(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::EraseAllDeletedRenderBlocks_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<RenderEngine> for RenderEngine {
    fn as_ref(&self) -> &RenderEngine {
        self
    }
}
impl std::convert::AsMut<RenderEngine> for RenderEngine {
    fn as_mut(&mut self) -> &mut RenderEngine {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The flat, contiguous render-pass id enum (ERenderPass). Every `pass` / `first` / `last` index in
/// the render engine is one of these. The pass-index ranges the render engine draws by:
/// GBuffer = RP_Z_OCCLUDERS..RP_LAST_GBUFFER, scene = RP_REFLECTIVE_WATER_PLANES..RP_LAST_MAIN,
/// post-effects = RP_POSTEFFECTS. Named `RenderPassId` to avoid clashing with the `RenderPass` type
/// (the CRenderPass class in render_pass.pyxis).
pub enum RenderPassId {
    RP_NONE = 0isize as _,
    RP_TERRAINPATCH_CLEAR = 1isize as _,
    RP_TERRAINPATCH_HISTOGRAM = 2isize as _,
    RP_TERRAINPATCH_TRIANGLES = 3isize as _,
    RP_TERRAINPATCH_SETUPDETAIL = 4isize as _,
    RP_TERRAINPATCH_SETUP = 5isize as _,
    RP_TERRAINPATCH_MORPH_TARGET = 6isize as _,
    RP_TERRAINPATCH_ENUMERATION = 7isize as _,
    PRE_RP_SKY_LIGHTING = 8isize as _,
    PRE_RP_REFLECTION_PRE = 9isize as _,
    PRE_RP_REFLECTION_DISTANT_BACKDROP = 10isize as _,
    PRE_RP_REFLECTION_ATMOSPHERE = 11isize as _,
    PRE_RP_REFLECTION_CLOUDS = 12isize as _,
    PRE_RP_REFLECTION_DETAIL_BACKDROP = 13isize as _,
    PRE_RP_REFLECTION_MESH = 14isize as _,
    PRE_RP_REFLECTION_DISTANT_LIGHTS = 15isize as _,
    PRE_RP_REFLECTION_POST = 16isize as _,
    PRE_RP_ENVREFLECTION = 17isize as _,
    PRE_RP_CLOUDSHADOWS = 18isize as _,
    PRE_RP_VEGETATION_UPDATE = 19isize as _,
    PRE_RP_VEG_INT_RECENTER = 20isize as _,
    PRE_RP_VEGETATION_INTERACTION = 21isize as _,
    PRE_RP_STATIC_SHADOW_0 = 22isize as _,
    PRE_RP_STATIC_SHADOW_1 = 23isize as _,
    PRE_RP_STATIC_SHADOW_2 = 24isize as _,
    PRE_RP_STATIC_SHADOW_3 = 25isize as _,
    PRE_RP_STATIC_SHADOW_4 = 26isize as _,
    PRE_RP_STATIC_SHADOW_5 = 27isize as _,
    PRE_RP_STATIC_SHADOW_6 = 28isize as _,
    PRE_RP_STATIC_SHADOW_7 = 29isize as _,
    PRE_RP_SHADOW_0 = 30isize as _,
    PRE_RP_SHADOW_1 = 31isize as _,
    PRE_RP_SHADOW_2 = 32isize as _,
    PRE_RP_SHADOW_3 = 33isize as _,
    PRE_RP_SHADOW_4 = 34isize as _,
    PRE_RP_SHADOW_5 = 35isize as _,
    PRE_RP_SHADOW_6 = 36isize as _,
    PRE_RP_SHADOW_7 = 37isize as _,
    PRE_RP_SHADOW_REFLECTIVE_SUN_NEAR = 38isize as _,
    PRE_RP_SHADOW_REFLECTIVE_SUN_FAR = 39isize as _,
    PRE_RP_SHADOW_REFLECTIVE_CAMERA = 40isize as _,
    PRE_RP_WATER_CS_PRE = 41isize as _,
    PRE_RP_WATER_WAKES_PRE = 42isize as _,
    PRE_RP_WATER_FOAM_PRE = 43isize as _,
    PRE_RP_WATER_DISPLACEMENT_PRE = 44isize as _,
    RP_RAIN_OCCLUDER = 45isize as _,
    PRE_RP_LAST_PREPASS = 46isize as _,
    RP_Z_OCCLUDERS = 47isize as _,
    RP_Z_COARSE_PASS = 48isize as _,
    RP_Z_PASS = 49isize as _,
    RP_Z_AND_VELOCITY_PASS = 50isize as _,
    RP_Z_DEBUG_VISUALIZATION = 51isize as _,
    RP_CLEAR = 52isize as _,
    RP_ROAD_STENCIL = 53isize as _,
    RP_TERRAINPATCH_DETAIL_MID = 54isize as _,
    RP_TERRAINPATCH_DETAIL_LOW = 55isize as _,
    RP_TERRAINPATCH_BASEMESH_TESSELLATE_NEAR = 56isize as _,
    RP_TERRAINPATCH_BASEMESH_NEAR = 57isize as _,
    RP_TERRAINPATCH_BASEMESH_TESSELLATE_FAR = 58isize as _,
    RP_TERRAINPATCH_BASEMESH_FAR = 59isize as _,
    RP_TERRAINPATCH_BASEMESH_TESSELLATE_COLOR = 60isize as _,
    RP_TERRAINPATCH_BASEMESH_COLOR = 61isize as _,
    RP_TERRAIN_APPLY_NEAR_DETAILED = 62isize as _,
    RP_TERRAIN_APPLY_NEAR = 63isize as _,
    RP_TERRAIN_APPLY_FAR = 64isize as _,
    RP_MODELS_DYNAMIC = 65isize as _,
    RP_MODELS_DYNAMIC_MASK_DAMAGE_POST_EFFECT = 66isize as _,
    RP_MODELS_STATIC = 67isize as _,
    RP_MODELS_REFLECTION = 68isize as _,
    RP_UNDERWATER_VEGETATION = 69isize as _,
    RP_VEGETATION_OPAQUE = 70isize as _,
    RP_VEGETATIONFINS = 71isize as _,
    RP_VEGETATIONGROUP = 72isize as _,
    RP_VEGETATIONGROUP2 = 73isize as _,
    RP_TERRAIN_FOREST = 74isize as _,
    RP_CREATURES = 75isize as _,
    RP_UNDERWATER_FOG_GRADIENT = 76isize as _,
    RP_Z_LOCK = 77isize as _,
    RP_ROAD_JUNCTION = 78isize as _,
    RP_ROAD_LAYERS = 79isize as _,
    RP_ROAD_JUNCTION_OPAQUE = 80isize as _,
    RP_DOWNSAMPLE_DEPTH = 81isize as _,
    RP_DECALS = 82isize as _,
    RP_SCREEN_SPACE_DECALS = 83isize as _,
    RP_SCREEN_SPACE_ROAD_DECALS = 84isize as _,
    RP_LAST_GBUFFER = 85isize as _,
    RP_REFLECTIVE_WATER_PLANES = 86isize as _,
    RP_AO_VOLUMES = 87isize as _,
    RP_SSAO = 88isize as _,
    RP_SCREEN_SPACE_REFLECTIONS = 89isize as _,
    RP_GLOBAL_ILLUMINATION = 90isize as _,
    RP_SCREEN_SPACE_SUBSURFACE_SKIN = 91isize as _,
    RP_DEFERRED_LIGHTS = 92isize as _,
    RP_DEBUG_GI = 93isize as _,
    RP_LINES = 94isize as _,
    RP_OCCLUDERS_DEBUG = 95isize as _,
    RP_BILLBOARD = 96isize as _,
    RP_OCCLUSION_QUERY = 97isize as _,
    RP_LAST_OPAQUE = 98isize as _,
    RP_STARS = 99isize as _,
    RP_SUN = 100isize as _,
    RP_MOON = 101isize as _,
    RP_SKYBOX = 102isize as _,
    RP_SKY_GRADIENT = 103isize as _,
    RP_FOG_GRADIENT = 104isize as _,
    RP_DEBUG_TRANSPARENCY = 105isize as _,
    RP_UNDERWATER_CLOUDS = 106isize as _,
    RP_UNDERWATER_VEGETATION_TRANSPARENT = 107isize as _,
    RP_COPY_FRAMEBUFFER = 108isize as _,
    RP_WATER = 109isize as _,
    RP_POST_WATER = 110isize as _,
    RP_SKIDMARKS = 111isize as _,
    RP_PRE_CLOUDS = 112isize as _,
    RP_LENSFLARE = 113isize as _,
    RP_POST_CLOUDS = 114isize as _,
    RP_APPLY_CLOUDS = 115isize as _,
    RP_FOG_VOLUME_GENERATE = 116isize as _,
    RP_FOG_VOLUME_UPSAMPLE = 117isize as _,
    RP_FOG_VOLUME_APPLY = 118isize as _,
    RP_MASK_WATER = 119isize as _,
    RP_MODELS_TRANSPARENT = 120isize as _,
    RP_VEGETATION_TRANSPARENT = 121isize as _,
    RP_VEGETATION_POST_DRAW = 122isize as _,
    RP_BB_RAIN = 123isize as _,
    RP_MODELS_GLINT = 124isize as _,
    RP_WATER_GODRAYS = 125isize as _,
    RP_BULLETS = 126isize as _,
    RP_CONTRAILS = 127isize as _,
    RP_GROUNDHAZE = 128isize as _,
    RP_PARTICLE_RIBBON = 129isize as _,
    RP_MODEL_HALO_POST = 130isize as _,
    RP_PARTICLE_LOWRES = 131isize as _,
    RP_SPOTLIGHT_VOLUMETRICS = 132isize as _,
    RP_WINDOW_DECALS = 133isize as _,
    RP_MODELS_REFRACT = 134isize as _,
    RP_PARTICLE_GENERAL = 135isize as _,
    RP_PARTICLE_DISTORT = 136isize as _,
    RP_PARTICLE_LOWRES_OVERLAY = 137isize as _,
    RP_SCENE_CAPTURE = 138isize as _,
    RP_Z_FINAL_TRANSPARENT = 139isize as _,
    RP_CLEAR_SCREEN_SPACE_SUBSURFACE_SKIN = 140isize as _,
    RP_CLEAR_STENCIL = 141isize as _,
    RP_GHOST_EFFECT = 142isize as _,
    RP_OUTLINE_MASK = 143isize as _,
    RP_OUTLINE_EFFECT = 144isize as _,
    RP_OUTLINE_EFFECT_NO_DEPTH = 145isize as _,
    RP_OUTLINE_EFFECT_BLUR = 146isize as _,
    RP_FINAL_TRANSPARENT = 147isize as _,
    RP_PARTICLE_ONSCREEN = 148isize as _,
    RP_POSTEFFECTS = 149isize as _,
    RP_LAST_MAIN = 150isize as _,
    POST_RP_FULLSCREEN_VIDEO = 151isize as _,
    RP_VEGETATION_SAMPLING = 152isize as _,
    POST_RP_POSTEFFECTS_GLOBAL = 153isize as _,
    POST_RP_UI = 154isize as _,
    POST_RP_DEBUG_GFX = 155isize as _,
    RP_RENDERPASS_COUNT = 156isize as _,
}
fn _RenderPassId_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], RenderPassId>([0u8; 0x4]);
    }
    unreachable!()
}
