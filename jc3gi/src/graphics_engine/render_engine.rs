#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The abstract render-block type interface (`NGraphicsEngine::IRenderBlockType`): the per-type
/// singleton every render block's `GetType` returns, holding the type's shaders and per-pass setup.
/// Mapped by vtable only.
pub struct RenderBlockTypeBase {
    vftable: *const crate::graphics_engine::render_engine::RenderBlockTypeBaseVftable,
}
fn _RenderBlockTypeBase_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], RenderBlockTypeBase>([0u8; 0x8]);
    }
    unreachable!()
}
impl RenderBlockTypeBase {
    pub fn vftable(
        &self,
    ) -> *const crate::graphics_engine::render_engine::RenderBlockTypeBaseVftable {
        self.vftable
            as *const crate::graphics_engine::render_engine::RenderBlockTypeBaseVftable
    }
    /// Creates the type's GPU resources (shaders, buffers) against the given
    /// `SResourceContext`. Each type's `RegisterType` calls this at startup with the render
    /// engine's own resource context.
    pub unsafe fn Create(
        &mut self,
        resource_context: *mut crate::graphics_engine::render_engine::ResourceContext,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).Create).read();
            f(self as *mut Self as _, resource_context)
        }
    }
    /// Destroys the type's GPU resources.
    pub unsafe fn Destroy(
        &mut self,
        resource_context: *mut crate::graphics_engine::render_engine::ResourceContext,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).Destroy).read();
            f(self as *mut Self as _, resource_context)
        }
    }
    /// Recreates the type's GPU resources against the given `SResourceContext`.
    /// `CRenderEngine::RecreateRenderBlockTypes` calls this on every registered type with the
    /// render engine's own resource context (the settings-change path) — but several types,
    /// including the terrain setup types, implement it as a no-op; re-creating those requires
    /// calling [`Destroy`](RenderBlockTypeBase::Destroy) and
    /// [`Create`](RenderBlockTypeBase::Create) directly.
    pub unsafe fn Recreate(
        &mut self,
        resource_context: *mut crate::graphics_engine::render_engine::ResourceContext,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).Recreate).read();
            f(self as *mut Self as _, resource_context)
        }
    }
    /// Returns the type's display name (e.g. `"VolumetricTerrain"`, `"TerrainPatch"`).
    pub unsafe fn GetTypeName(&self) -> *const u8 {
        unsafe {
            let f = (&raw const (*self.vftable()).GetTypeName).read();
            f(self as *const Self as _)
        }
    }
    /// Returns the type's name hash (the registry sort key).
    pub unsafe fn GetHash(&self) -> u32 {
        unsafe {
            let f = (&raw const (*self.vftable()).GetHash).read();
            f(self as *const Self as _)
        }
    }
    /// Whether render passes draw blocks of this type: `CRenderPass::DoDraw` dispatches this
    /// per type run (vtable offset `0x90`) and skips every block whose type reports disabled.
    /// In the release build the base implementation is compiled to a constant `true`.
    pub unsafe fn IsEnabled(&self) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).IsEnabled).read();
            f(self as *const Self as _)
        }
    }
    /// Enables drawing of this type's blocks. In the release build the base implementation is
    /// compiled to a no-op (the enabled flag was optimized out).
    pub unsafe fn Enable(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).Enable).read();
            f(self as *mut Self as _)
        }
    }
    /// Disables drawing of this type's blocks. In the release build the base implementation is
    /// compiled to a no-op (the enabled flag was optimized out), so suppressing a type requires
    /// replacing its [`IsEnabled`](RenderBlockTypeBase::IsEnabled) vtable entry.
    pub unsafe fn Disable(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).Disable).read();
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<RenderBlockTypeBase> for RenderBlockTypeBase {
    fn as_ref(&self) -> &RenderBlockTypeBase {
        self
    }
}
impl std::convert::AsMut<RenderBlockTypeBase> for RenderBlockTypeBase {
    fn as_mut(&mut self) -> &mut RenderBlockTypeBase {
        self
    }
}
#[repr(C, align(8))]
pub struct RenderBlockTypeBaseVftable {
    _vfunc_0: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
    ),
    /// Creates the type's GPU resources (shaders, buffers) against the given
    /// `SResourceContext`. Each type's `RegisterType` calls this at startup with the render
    /// engine's own resource context.
    pub Create: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
        resource_context: *mut crate::graphics_engine::render_engine::ResourceContext,
    ),
    /// Destroys the type's GPU resources.
    pub Destroy: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
        resource_context: *mut crate::graphics_engine::render_engine::ResourceContext,
    ),
    /// Recreates the type's GPU resources against the given `SResourceContext`.
    /// `CRenderEngine::RecreateRenderBlockTypes` calls this on every registered type with the
    /// render engine's own resource context (the settings-change path) — but several types,
    /// including the terrain setup types, implement it as a no-op; re-creating those requires
    /// calling [`Destroy`](RenderBlockTypeBase::Destroy) and
    /// [`Create`](RenderBlockTypeBase::Create) directly.
    pub Recreate: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
        resource_context: *mut crate::graphics_engine::render_engine::ResourceContext,
    ),
    /// Returns the type's display name (e.g. `"VolumetricTerrain"`, `"TerrainPatch"`).
    pub GetTypeName: unsafe extern "system" fn(
        this: *const crate::graphics_engine::render_engine::RenderBlockTypeBase,
    ) -> *const u8,
    /// Returns the type's name hash (the registry sort key).
    pub GetHash: unsafe extern "system" fn(
        this: *const crate::graphics_engine::render_engine::RenderBlockTypeBase,
    ) -> u32,
    _vfunc_6: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
    ),
    _vfunc_7: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
    ),
    _vfunc_8: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
    ),
    _vfunc_9: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
    ),
    _vfunc_10: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
    ),
    _vfunc_11: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
    ),
    _vfunc_12: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
    ),
    _vfunc_13: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
    ),
    _vfunc_14: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
    ),
    _vfunc_15: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
    ),
    _vfunc_16: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
    ),
    _vfunc_17: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
    ),
    /// Whether render passes draw blocks of this type: `CRenderPass::DoDraw` dispatches this
    /// per type run (vtable offset `0x90`) and skips every block whose type reports disabled.
    /// In the release build the base implementation is compiled to a constant `true`.
    pub IsEnabled: unsafe extern "system" fn(
        this: *const crate::graphics_engine::render_engine::RenderBlockTypeBase,
    ) -> bool,
    /// Enables drawing of this type's blocks. In the release build the base implementation is
    /// compiled to a no-op (the enabled flag was optimized out).
    pub Enable: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
    ),
    /// Disables drawing of this type's blocks. In the release build the base implementation is
    /// compiled to a no-op (the enabled flag was optimized out), so suppressing a type requires
    /// replacing its [`IsEnabled`](RenderBlockTypeBase::IsEnabled) vtable entry.
    pub Disable: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
    ),
}
fn _RenderBlockTypeBaseVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xA8], RenderBlockTypeBaseVftable>([0u8; 0xA8]);
    }
    unreachable!()
}
impl RenderBlockTypeBaseVftable {}
impl std::convert::AsRef<RenderBlockTypeBaseVftable> for RenderBlockTypeBaseVftable {
    fn as_ref(&self) -> &RenderBlockTypeBaseVftable {
        self
    }
}
impl std::convert::AsMut<RenderBlockTypeBaseVftable> for RenderBlockTypeBaseVftable {
    fn as_mut(&mut self) -> &mut RenderBlockTypeBaseVftable {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(8))]
/// One entry in the global render-block-type registry: the type's hash (its `GetHash`) and the
/// type object.
pub struct RenderBlockTypeEntry {
    pub m_Hash: u32,
    _field_4: [u8; 4],
    pub m_Type: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
}
fn _RenderBlockTypeEntry_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], RenderBlockTypeEntry>([0u8; 0x10]);
    }
    unreachable!()
}
impl RenderBlockTypeEntry {}
impl std::convert::AsRef<RenderBlockTypeEntry> for RenderBlockTypeEntry {
    fn as_ref(&self) -> &RenderBlockTypeEntry {
        self
    }
}
impl std::convert::AsMut<RenderBlockTypeEntry> for RenderBlockTypeEntry {
    fn as_mut(&mut self) -> &mut RenderBlockTypeEntry {
        self
    }
}
#[repr(C, align(8))]
/// The global render-block-type registry that `CRenderEngine::AddType` and `RemoveType` maintain
/// (the leading fields of the `CRenderBlockFactory` object): a vector of
/// [`RenderBlockTypeEntry`], kept sorted by type hash for binary search. The factory itself sits
/// behind a pointer in static storage.
pub struct RenderBlockTypeRegistry {
    pub m_Types: crate::types::std_vector::Vector<
        crate::graphics_engine::render_engine::RenderBlockTypeEntry,
    >,
}
fn _RenderBlockTypeRegistry_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], RenderBlockTypeRegistry>([0u8; 0x20]);
    }
    unreachable!()
}
impl RenderBlockTypeRegistry {}
impl std::convert::AsRef<RenderBlockTypeRegistry> for RenderBlockTypeRegistry {
    fn as_ref(&self) -> &RenderBlockTypeRegistry {
        self
    }
}
impl std::convert::AsMut<RenderBlockTypeRegistry> for RenderBlockTypeRegistry {
    fn as_mut(&mut self) -> &mut RenderBlockTypeRegistry {
        self
    }
}
#[repr(C, align(8))]
pub struct RenderEngine {
    _field_0: [u8; 128],
    /// The per-pass render-block-item lists: one vector of [`RenderPass`] pointers per pass id.
    /// [`DrawRenderPassRange`](RenderEngine::DrawRenderPassRange) and the per-frame list rotation walk
    /// this.
    pub m_RenderPasses: [crate::types::std_vector::Vector<
        *mut crate::graphics_engine::render_pass::RenderPass,
    >; 157],
    _field_1420: [u8; 672],
    /// The per-Draw constant-buffer ring index (feeding `CalculateConstantBufferIndices`): each `Draw`
    /// selects a constant-buffer pool slot from this and advances it, wrapping at the limit in the `u32`
    /// immediately after. It advances independently of the engine frame counters.
    pub m_ConstantBufferRingIndex: u32,
    _field_16c4: [u8; 524],
    /// The render engine's embedded [`ResourceContext`]. `RecreateRenderBlockTypes` passes a
    /// pointer to this field to every type's [`Recreate`](RenderBlockTypeBase::Recreate), and each
    /// type's `RegisterType` passes it to [`Create`](RenderBlockTypeBase::Create) at startup.
    pub m_ResourceContext: crate::graphics_engine::render_engine::ResourceContext,
    _field_18f0: [u8; 2352],
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
    /// The late render-pass step: finalizes and copies render targets under the context mutex.
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
    /// Draws every render block in the half-open pass-index range `[first, last)`: for each pass it
    /// walks the [`RenderPass`] list and vtable-dispatches each block.
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
    pub const PreDraw_ADDRESS: usize = 0x140186760;
    /// The pre-pass step: iterates the pre-pass categories (`1..=45` -- terrain-patch prep, the sky
    /// lighting LUT, planar and environment reflections, cloud shadows, vegetation, the static and
    /// dynamic sun-shadow cascade atlas, the reflective-shadow passes, the water-simulation compute, and
    /// the rain occluder) and vtable-dispatches each [`RenderPass`]'s `Draw`, feeding it the render
    /// context. Called from `HandleDrawThreadTask` before the GBuffer range. Each pass's
    /// [`m_Enabled`](graphics_engine::render_pass::RenderPassState::m_Enabled) gates whether it draws;
    /// the pre-pass cameras are the sun / reflection / world-space cameras (own camera, `RenderPass +
    /// 0x870`), never the per-eye render camera, except terrain-patch prep (`1..=7`), which falls
    /// through to the render camera.
    pub unsafe fn PreDraw(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
    ) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
            ) -> u64 = ::std::mem::transmute(Self::PreDraw_ADDRESS);
            f(self as *mut Self as _, ctx)
        }
    }
    pub const DrawGBuffer_ADDRESS: usize = 0x140186810;
    /// The GBuffer fill: binds two global textures, then draws the GBuffer pass range (the depth and
    /// velocity prefix, static and dynamic models, and decals).
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
    /// Lighting, reflections, opaque, environment, water, and transparency: draws the scene pass
    /// range, then clears the global texture samplers.
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
    /// The post-effects pass: draws the `RP_POSTEFFECTS` range, whose block is
    /// [`RenderBlockPostEffects::Draw`].
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
    /// render camera's full (translation-bearing) view-projection and world position. This drives
    /// screen-space and non-geometry work, not opaque-geometry vertex placement.
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
    /// The per-frame TAA jitter: forwards to [`PostEffectsManager::ApplySubsampleJitter`], which
    /// post-multiplies a sub-pixel clip-space translation onto `proj` only at the T2X resolve mode.
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
    /// Drains a separate deferred deletion list of render blocks, under its own critical section. Does
    /// not touch the per-pass draw lists.
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
/// The flat, contiguous render-pass id enum. Every pass / first / last index in the render engine is
/// one of these. The render engine draws by pass-index range: the GBuffer from `RP_Z_OCCLUDERS` to
/// `RP_LAST_GBUFFER`, the scene from `RP_REFLECTIVE_WATER_PLANES` to `RP_LAST_MAIN`, and the
/// post-effects at `RP_POSTEFFECTS`. Named [`RenderPassId`] to avoid clashing with the [`RenderPass`]
/// type.
///
/// Verified against the retail pass-name switch ([`GetRenderPassName`]): relative to the 2016 dump,
/// retail inserts `RP_VEGETATION_TRANSPARENT_AOIT` at `0x74` (shifting everything above by one) and
/// removes the dump-era `RP_PARTICLE_RIBBON`, leaving `0x82` unnamed.
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
    RP_VEGETATION_TRANSPARENT_AOIT = 116isize as _,
    RP_FOG_VOLUME_GENERATE = 117isize as _,
    RP_FOG_VOLUME_UPSAMPLE = 118isize as _,
    RP_FOG_VOLUME_APPLY = 119isize as _,
    RP_MASK_WATER = 120isize as _,
    RP_MODELS_TRANSPARENT = 121isize as _,
    RP_VEGETATION_TRANSPARENT = 122isize as _,
    RP_VEGETATION_POST_DRAW = 123isize as _,
    RP_BB_RAIN = 124isize as _,
    RP_MODELS_GLINT = 125isize as _,
    RP_WATER_GODRAYS = 126isize as _,
    RP_BULLETS = 127isize as _,
    RP_CONTRAILS = 128isize as _,
    RP_GROUNDHAZE = 129isize as _,
    RP_MODEL_HALO_POST = 131isize as _,
    RP_PARTICLE_LOWRES = 132isize as _,
    RP_SPOTLIGHT_VOLUMETRICS = 133isize as _,
    RP_WINDOW_DECALS = 134isize as _,
    RP_MODELS_REFRACT = 135isize as _,
    RP_PARTICLE_GENERAL = 136isize as _,
    RP_PARTICLE_DISTORT = 137isize as _,
    RP_PARTICLE_LOWRES_OVERLAY = 138isize as _,
    RP_SCENE_CAPTURE = 139isize as _,
    RP_Z_FINAL_TRANSPARENT = 140isize as _,
    RP_CLEAR_SCREEN_SPACE_SUBSURFACE_SKIN = 141isize as _,
    RP_CLEAR_STENCIL = 142isize as _,
    RP_GHOST_EFFECT = 143isize as _,
    RP_OUTLINE_MASK = 144isize as _,
    RP_OUTLINE_EFFECT = 145isize as _,
    RP_OUTLINE_EFFECT_NO_DEPTH = 146isize as _,
    RP_OUTLINE_EFFECT_BLUR = 147isize as _,
    RP_FINAL_TRANSPARENT = 148isize as _,
    RP_PARTICLE_ONSCREEN = 149isize as _,
    RP_POSTEFFECTS = 150isize as _,
    RP_LAST_MAIN = 151isize as _,
    POST_RP_FULLSCREEN_VIDEO = 152isize as _,
    RP_VEGETATION_SAMPLING = 153isize as _,
    POST_RP_POSTEFFECTS_GLOBAL = 154isize as _,
    POST_RP_UI = 155isize as _,
    POST_RP_DEBUG_GFX = 156isize as _,
    RP_RENDERPASS_COUNT = 157isize as _,
}
fn _RenderPassId_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], RenderPassId>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// The resource-creation context (`NGraphicsEngine::SResourceContext`) handed to render-block
/// types' `Create`/`Destroy`/`Recreate`: the graphics device plus the texture and shader caches.
/// The render engine embeds one ([`RenderEngine::m_ResourceContext`]) that every type registration
/// and recreation path uses.
pub struct ResourceContext {
    pub m_GraphicsDevice: *mut crate::graphics_engine::graphics_engine::HDevice_t,
    pub m_TextureCache: *mut crate::graphics_engine::render_engine::TextureCache,
    pub m_ShaderCache: *mut crate::graphics_engine::render_engine::ShaderCache,
    /// The `Graphics::EMaxAniso` anisotropic-filtering level.
    pub m_MaxAniso: i32,
    _field_1c: [u8; 4],
}
fn _ResourceContext_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], ResourceContext>([0u8; 0x20]);
    }
    unreachable!()
}
impl ResourceContext {}
impl std::convert::AsRef<ResourceContext> for ResourceContext {
    fn as_ref(&self) -> &ResourceContext {
        self
    }
}
impl std::convert::AsMut<ResourceContext> for ResourceContext {
    fn as_mut(&mut self) -> &mut ResourceContext {
        self
    }
}
#[repr(C, align(8))]
/// The terrain patch render system's per-frame state (partial: only the fields walked to
/// `m_TerrainCamera` are mapped). Owned by `CLandscapeManager`; updated by
/// [`TerrainPatchSystemUpdate`].
pub struct STerrainPatchSystem {
    _field_0: [u8; 80],
    /// Whether [`TerrainPatchSystemUpdate`] refreshes
    /// [`m_TerrainCamera`](STerrainPatchSystem::m_TerrainCamera) from the LOD camera this frame; when
    /// clear, the previous frame's copy is kept.
    pub m_UpdateCamera: bool,
    _field_51: [u8; 3],
    /// The camera-space distance at which terrain patches subdivide (tessellation LOD).
    pub m_TessellationDistance: f32,
    /// A copy of the LOD camera (`CameraManager::m_ActiveCamera`) taken when
    /// [`m_UpdateCamera`](STerrainPatchSystem::m_UpdateCamera) is set. The terrain render passes point
    /// their frustum camera at this and cull patches against its
    /// [`m_FrustumPlane`](camera::camera::Camera::m_FrustumPlane).
    pub m_TerrainCamera: crate::camera::camera::Camera,
}
fn _STerrainPatchSystem_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x608], STerrainPatchSystem>([0u8; 0x608]);
    }
    unreachable!()
}
impl STerrainPatchSystem {}
impl std::convert::AsRef<STerrainPatchSystem> for STerrainPatchSystem {
    fn as_ref(&self) -> &STerrainPatchSystem {
        self
    }
}
impl std::convert::AsMut<STerrainPatchSystem> for STerrainPatchSystem {
    fn as_mut(&mut self) -> &mut STerrainPatchSystem {
        self
    }
}
#[repr(C, align(8))]
/// The opaque shader cache (`SShaderCache`), the name-hash-keyed store the `ShaderCacheGet*Program`
/// family resolves shader holders from.
pub struct ShaderCache {}
impl ShaderCache {}
impl std::convert::AsRef<ShaderCache> for ShaderCache {
    fn as_ref(&self) -> &ShaderCache {
        self
    }
}
impl std::convert::AsMut<ShaderCache> for ShaderCache {
    fn as_mut(&mut self) -> &mut ShaderCache {
        self
    }
}
#[repr(C, align(8))]
/// The immediate operands inside the terrain setup types' `Create` functions that size the GPU
/// detail-tessellation budget buffers (see `CRenderBlockTerrainDetailSetup::Create` at
/// `0x14032C000` and `CRenderBlockTerrainSetup::Create` at `0x14032EA10`). The detail-quad
/// pipeline allocates from these buffers with unbounded GPU cursors, so their sizes are the hard
/// budget of the detail tessellation skin; each constant is the address of a 32-bit element-count
/// immediate, paired with the shipped value.
pub struct TerrainDetailBudgetPatchSites {}
impl TerrainDetailBudgetPatchSites {}
impl TerrainDetailBudgetPatchSites {
    /// "Detail debug tessellation vertex buffer": 0x10000 elements of 80 bytes.
    pub const DEBUG_VERTEX_COUNT: u64 = 5372035357;
    /// "Detail tessellation index texture buffer" (the raw index buffer): 0x40000 bytes.
    pub const INDEX_BYTES: u64 = 5372035429;
    /// "Detail tessellation index texture buffer" (the 4-byte-typed view on the setup type):
    /// 0x8000 elements.
    pub const INDEX_VIEW_COUNT: u64 = 5372046286;
    /// "Detail tessellation texel buffer": 0x8000 elements of 16 bytes.
    pub const TEXEL_COUNT: u64 = 5372035501;
    /// "Detail tessellation vertex buffer": 0x10000 elements of 16 bytes.
    pub const VERTEX_COUNT: u64 = 5372035278;
}
impl std::convert::AsRef<TerrainDetailBudgetPatchSites>
for TerrainDetailBudgetPatchSites {
    fn as_ref(&self) -> &TerrainDetailBudgetPatchSites {
        self
    }
}
impl std::convert::AsMut<TerrainDetailBudgetPatchSites>
for TerrainDetailBudgetPatchSites {
    fn as_mut(&mut self) -> &mut TerrainDetailBudgetPatchSites {
        self
    }
}
#[repr(C, align(8))]
/// The opaque texture cache (`STextureCache`).
pub struct TextureCache {}
impl TextureCache {}
impl std::convert::AsRef<TextureCache> for TextureCache {
    fn as_ref(&self) -> &TextureCache {
        self
    }
}
impl std::convert::AsMut<TextureCache> for TextureCache {
    fn as_mut(&mut self) -> &mut TextureCache {
        self
    }
}
pub const GetRenderPassName_ADDRESS: usize = 0x140175080;
/// The debug name for a render-pass id, from the engine's pass-name switch (the ground truth the
/// [`RenderPassId`] values are verified against). Returns `"NONE"` for unnamed indices.
pub unsafe fn GetRenderPassName(
    pass: crate::graphics_engine::render_engine::RenderPassId,
) -> *const u8 {
    unsafe {
        let f: unsafe extern "system" fn(
            pass: crate::graphics_engine::render_engine::RenderPassId,
        ) -> *const u8 = ::std::mem::transmute(GetRenderPassName_ADDRESS);
        f(pass)
    }
}
pub const TerrainPatchSystemUpdate_ADDRESS: usize = 0x14032F780;
/// The once-per-frame terrain patch system update (called from `CLandscapeManager::UpdateRender` in the
/// sim phase). When [`STerrainPatchSystem::m_UpdateCamera`] is set it copies the LOD camera
/// (`CameraManager::m_ActiveCamera`) into [`STerrainPatchSystem::m_TerrainCamera`] and points every
/// terrain render pass's frustum camera at it. `ctx` carries the source camera.
pub unsafe fn TerrainPatchSystemUpdate(
    handle: *mut crate::graphics_engine::render_engine::STerrainPatchSystem,
    ctx: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            handle: *mut crate::graphics_engine::render_engine::STerrainPatchSystem,
            ctx: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(TerrainPatchSystemUpdate_ADDRESS);
        f(handle, ctx)
    }
}
impl RenderBlockTypeBase {
    /// The type's display name as a string, or `None` when the vtable returns null or a non-UTF-8
    /// name. The borrow is `'static`: the engine's type names are string literals in the module
    /// image.
    ///
    /// # Safety
    /// `self` must be a live type object with a valid vtable.
    pub unsafe fn get_type_name_str(&self) -> Option<&'static str> {
        unsafe {
            let ptr = self.GetTypeName();
            if ptr.is_null() {
                return None;
            }
            std::ffi::CStr::from_ptr(ptr.cast()).to_str().ok()
        }
    }
}
impl RenderBlockTypeRegistry {
    /// The live registry: the static slot holds a pointer to the factory object whose leading
    /// fields are the vector.
    pub unsafe fn get() -> Option<&'static mut RenderBlockTypeRegistry> {
        unsafe { (*(0x142ED0F60usize as *const *mut RenderBlockTypeRegistry)).as_mut() }
    }
    /// The registered types as a slice.
    ///
    /// # Safety
    /// The registry's element range must point to live entries for the borrow.
    pub unsafe fn as_slice(&self) -> &[RenderBlockTypeEntry] {
        unsafe { self.m_Types.as_slice() }
    }
}
