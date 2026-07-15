#![cfg_attr(any(), rustfmt::skip)]
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// A 3x4 skinning-palette bone matrix. The layout differs per render block (empirically, by the
/// block's vertex format/shader variant): some blocks store four 3-float columns with the
/// translation in the final three floats, others three 4-float rows with the translation in each
/// row's fourth element. The 3x3 rotation is orthonormal under the correct reading, which is how
/// a consumer can detect the layout. The skinning palette is an array of these, one per skeleton
/// bone, built per frame by `CPoseProducer::MakeSkinningPalette`.
pub struct Matrix3x4 {
    pub m: [f32; 12],
}
fn _Matrix3x4_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], Matrix3x4>([0u8; 0x30]);
    }
    unreachable!()
}
impl Matrix3x4 {}
impl std::convert::AsRef<Matrix3x4> for Matrix3x4 {
    fn as_ref(&self) -> &Matrix3x4 {
        self
    }
}
impl std::convert::AsMut<Matrix3x4> for Matrix3x4 {
    fn as_mut(&mut self) -> &mut Matrix3x4 {
        self
    }
}
#[repr(C, align(8))]
/// The per-draw render block instance info: the instance's constant buffers, LOD state, and world
/// transforms.
pub struct RBIInfo {}
impl RBIInfo {
    pub const GetMatrix_ADDRESS: usize = 0x1400B1850;
    /// Writes the instance world transform for the given transform slot into `out` (also returned).
    /// The render blocks pass [`RenderContext::m_TransformIndex`](graphics_engine::graphics_engine::RenderContext::m_TransformIndex)
    /// as the slot for the current dispatch.
    pub unsafe fn GetMatrix(
        &self,
        out: *mut crate::types::math::Matrix4,
        index: i32,
    ) -> *mut crate::types::math::Matrix4 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                out: *mut crate::types::math::Matrix4,
                index: i32,
            ) -> *mut crate::types::math::Matrix4 = ::std::mem::transmute(
                Self::GetMatrix_ADDRESS,
            );
            f(self as *const Self as _, out, index)
        }
    }
}
impl std::convert::AsRef<RBIInfo> for RBIInfo {
    fn as_ref(&self) -> &RBIInfo {
        self
    }
}
impl std::convert::AsMut<RBIInfo> for RBIInfo {
    fn as_mut(&mut self) -> &mut RBIInfo {
        self
    }
}
#[repr(C, align(8))]
/// The atmospheric-scattering / aerial-perspective render block. Its `Draw` reconstructs world
/// position from depth via [`Matrix4::PerspectiveFovInverse`](types::math::Matrix4) -- for the whole
/// screen, sky included -- and then ray-marches the sun shadow cascade and aerial perspective over
/// the reconstructed positions.
pub struct RenderBlockAtmosphericScattering {}
impl RenderBlockAtmosphericScattering {
    pub const Draw_ADDRESS: usize = 0x14036A820;
    /// Draws the atmospheric-scattering pass. `rc` is the per-view render context; `info` the
    /// instance info. Reconstructs view rays from depth via
    /// [`Matrix4::PerspectiveFovInverse`](types::math::Matrix4) and samples the sun cascade.
    pub unsafe fn Draw(
        &mut self,
        rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
        info: *const crate::graphics_engine::render_block::RBIInfo,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
                info: *const crate::graphics_engine::render_block::RBIInfo,
            ) = ::std::mem::transmute(Self::Draw_ADDRESS);
            f(self as *mut Self as _, rc, info)
        }
    }
}
impl std::convert::AsRef<RenderBlockAtmosphericScattering>
for RenderBlockAtmosphericScattering {
    fn as_ref(&self) -> &RenderBlockAtmosphericScattering {
        self
    }
}
impl std::convert::AsMut<RenderBlockAtmosphericScattering>
for RenderBlockAtmosphericScattering {
    fn as_mut(&mut self) -> &mut RenderBlockAtmosphericScattering {
        self
    }
}
#[repr(C, align(8))]
/// The skinned character render block (the `Character` RBMDL block type). A character model is
/// composed of one block per material; the same block objects are drawn for every pass, branching
/// internally on [`RenderContext::m_RenderStatus`](graphics_engine::graphics_engine::RenderContext::m_RenderStatus)
/// to select the shadow/depth-only path versus the full material path.
pub struct RenderBlockCharacter {
    _field_0: [u8; 584],
    /// The `std::vector<CSkinBatch>` begin pointer.
    pub m_SkinBatchesBegin: *mut crate::graphics_engine::render_block::SkinBatch,
    /// The `std::vector<CSkinBatch>` end pointer.
    pub m_SkinBatchesEnd: *mut crate::graphics_engine::render_block::SkinBatch,
}
fn _RenderBlockCharacter_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x258], RenderBlockCharacter>([0u8; 0x258]);
    }
    unreachable!()
}
impl RenderBlockCharacter {
    pub const Draw_ADDRESS: usize = 0x14013A310;
    /// Draws the block for the current pass. Shadow passes
    /// ([`RenderContext::m_RenderStatus`](graphics_engine::graphics_engine::RenderContext::m_RenderStatus) `& 6`)
    /// take a depth-only path with the depth vertex shaders; other passes run the full material
    /// setup.
    pub unsafe fn Draw(
        &self,
        rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
        info: *const crate::graphics_engine::render_block::RBIInfo,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
                info: *const crate::graphics_engine::render_block::RBIInfo,
            ) = ::std::mem::transmute(Self::Draw_ADDRESS);
            f(self as *const Self as _, rc, info)
        }
    }
    pub const DrawZ_ADDRESS: usize = 0x140139CD0;
    /// Draws the block for the Z/velocity prepass.
    pub unsafe fn DrawZ(
        &self,
        rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
        info: *const crate::graphics_engine::render_block::RBIInfo,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
                info: *const crate::graphics_engine::render_block::RBIInfo,
            ) = ::std::mem::transmute(Self::DrawZ_ADDRESS);
            f(self as *const Self as _, rc, info)
        }
    }
    pub const SetMatrixPalette_ADDRESS: usize = 0x140108200;
    /// Uploads one batch's bone matrices to the vertex-program palette constants: for each batch
    /// slot, copies `matrices[BatchToSkeletonLookup[slot]]` into the constant registers starting
    /// at `register`. Called from the block's internal `DrawBatches` before each batch's draw.
    pub unsafe fn SetMatrixPalette(
        &self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        matrices: *const crate::graphics_engine::render_block::Matrix3x4,
        batch: *const crate::graphics_engine::render_block::SkinBatch,
        register: u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
                matrices: *const crate::graphics_engine::render_block::Matrix3x4,
                batch: *const crate::graphics_engine::render_block::SkinBatch,
                register: u32,
            ) = ::std::mem::transmute(Self::SetMatrixPalette_ADDRESS);
            f(self as *const Self as _, ctx, matrices, batch, register)
        }
    }
}
impl std::convert::AsRef<RenderBlockCharacter> for RenderBlockCharacter {
    fn as_ref(&self) -> &RenderBlockCharacter {
        self
    }
}
impl std::convert::AsMut<RenderBlockCharacter> for RenderBlockCharacter {
    fn as_mut(&mut self) -> &mut RenderBlockCharacter {
        self
    }
}
#[repr(C, align(8))]
/// The skinned character skin render block (the `CharacterSkin` RBMDL block type): the skin-shaded
/// variant of [`RenderBlockCharacter`], with the same batch and pass structure.
pub struct RenderBlockCharacterSkin {
    _field_0: [u8; 448],
    /// The `std::vector<CSkinBatch>` begin pointer.
    pub m_SkinBatchesBegin: *mut crate::graphics_engine::render_block::SkinBatch,
    /// The `std::vector<CSkinBatch>` end pointer.
    pub m_SkinBatchesEnd: *mut crate::graphics_engine::render_block::SkinBatch,
}
fn _RenderBlockCharacterSkin_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1D0], RenderBlockCharacterSkin>([0u8; 0x1D0]);
    }
    unreachable!()
}
impl RenderBlockCharacterSkin {
    pub const Draw_ADDRESS: usize = 0x14013B580;
    /// Draws the block for the current pass; see [`RenderBlockCharacter::Draw`].
    pub unsafe fn Draw(
        &self,
        rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
        info: *const crate::graphics_engine::render_block::RBIInfo,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
                info: *const crate::graphics_engine::render_block::RBIInfo,
            ) = ::std::mem::transmute(Self::Draw_ADDRESS);
            f(self as *const Self as _, rc, info)
        }
    }
    pub const DrawZ_ADDRESS: usize = 0x14013AF60;
    /// Draws the block for the Z/velocity prepass.
    pub unsafe fn DrawZ(
        &self,
        rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
        info: *const crate::graphics_engine::render_block::RBIInfo,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
                info: *const crate::graphics_engine::render_block::RBIInfo,
            ) = ::std::mem::transmute(Self::DrawZ_ADDRESS);
            f(self as *const Self as _, rc, info)
        }
    }
    pub const SetMatrixPalette_ADDRESS: usize = 0x140108DD0;
    /// See [`RenderBlockCharacter::SetMatrixPalette`].
    pub unsafe fn SetMatrixPalette(
        &self,
        ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
        matrices: *const crate::graphics_engine::render_block::Matrix3x4,
        batch: *const crate::graphics_engine::render_block::SkinBatch,
        register: u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
                matrices: *const crate::graphics_engine::render_block::Matrix3x4,
                batch: *const crate::graphics_engine::render_block::SkinBatch,
                register: u32,
            ) = ::std::mem::transmute(Self::SetMatrixPalette_ADDRESS);
            f(self as *const Self as _, ctx, matrices, batch, register)
        }
    }
}
impl std::convert::AsRef<RenderBlockCharacterSkin> for RenderBlockCharacterSkin {
    fn as_ref(&self) -> &RenderBlockCharacterSkin {
        self
    }
}
impl std::convert::AsMut<RenderBlockCharacterSkin> for RenderBlockCharacterSkin {
    fn as_mut(&mut self) -> &mut RenderBlockCharacterSkin {
        self
    }
}
#[repr(C, align(8))]
/// The deferred-lighting render block. Its `Draw` method dispatches either the clustered (tiled)
/// lighting pass or a pass-through fallback.
pub struct RenderBlockDeferredLighting {}
impl RenderBlockDeferredLighting {
    pub const DrawClustered_ADDRESS: usize = 0x14013CFD0;
    /// The clustered-lighting entry point: runs the "LightAssignment" pass (rasterizing light proxy
    /// geometry into the froxel light-lookup target) and the "ClusteredLighting" pass (shading from
    /// it). Called from `Draw` when wireframe is disabled.
    pub unsafe fn DrawClustered(
        &self,
        rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
        a3: *mut ::std::ffi::c_void,
        a4: *mut crate::graphics_engine::graphics_engine::HTexture_t,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
                a3: *mut ::std::ffi::c_void,
                a4: *mut crate::graphics_engine::graphics_engine::HTexture_t,
            ) = ::std::mem::transmute(Self::DrawClustered_ADDRESS);
            f(self as *const Self as _, rc, a3, a4)
        }
    }
}
impl std::convert::AsRef<RenderBlockDeferredLighting> for RenderBlockDeferredLighting {
    fn as_ref(&self) -> &RenderBlockDeferredLighting {
        self
    }
}
impl std::convert::AsMut<RenderBlockDeferredLighting> for RenderBlockDeferredLighting {
    fn as_mut(&mut self) -> &mut RenderBlockDeferredLighting {
        self
    }
}
#[repr(C, align(8))]
/// The fog-volume render block *type* (the
/// `NGraphicsEngine::CRenderBlockFogVolume::CRenderBlockTypeFogVolume` singleton): owns the froxel
/// volumetric-fog textures and recreates them when the scene render resolution changes.
pub struct RenderBlockTypeFogVolume {
    _field_0: [u8; 296],
    /// The full-resolution fog target width, in pixels, latched from the last
    /// [`ResizeTextures`](RenderBlockTypeFogVolume::ResizeTextures) call.
    pub m_HiResTextureWidth: u32,
    /// The full-resolution fog target height, in pixels; see
    /// [`m_HiResTextureWidth`](RenderBlockTypeFogVolume::m_HiResTextureWidth).
    pub m_HiResTextureHeight: u32,
}
fn _RenderBlockTypeFogVolume_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x130], RenderBlockTypeFogVolume>([0u8; 0x130]);
    }
    unreachable!()
}
impl RenderBlockTypeFogVolume {
    pub const ResizeTextures_ADDRESS: usize = 0x14010C5A0;
    /// Recreates the fog-volume textures for a `width` x `height` render target: the full-resolution
    /// `fogvolume_texture_0` colour target and its volume texture, plus a coarse volumetric-depth
    /// buffer that is resized to *half* of `width` x `height`. Invoked from the graphics engine's
    /// registered resolution-change callback, so it re-runs whenever the scene render targets are
    /// recreated (a resolution change), not per frame.
    pub unsafe fn ResizeTextures(&mut self, width: u32, height: u32) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                width: u32,
                height: u32,
            ) -> bool = ::std::mem::transmute(Self::ResizeTextures_ADDRESS);
            f(self as *mut Self as _, width, height)
        }
    }
}
impl std::convert::AsRef<RenderBlockTypeFogVolume> for RenderBlockTypeFogVolume {
    fn as_ref(&self) -> &RenderBlockTypeFogVolume {
        self
    }
}
impl std::convert::AsMut<RenderBlockTypeFogVolume> for RenderBlockTypeFogVolume {
    fn as_mut(&mut self) -> &mut RenderBlockTypeFogVolume {
        self
    }
}
#[repr(C, align(8))]
/// The particle render block *type* (the `CRenderBlockParticle::CRenderBlockTypeParticle` singleton):
/// the shared state and shaders for every particle render block, including the flags that decide
/// whether a particle draw is routed to the low-resolution particle pass.
pub struct RenderBlockTypeParticle {
    _field_0: [u8; 2693],
    /// When set, a particle render block whose effect opts in and that falls below the low-resolution
    /// distance threshold routes its draw to the low-resolution particle pass (later composited back
    /// up by the low-res upsampling pass); when clear, that particle routes to the full-resolution
    /// transparent pass instead. Set from the particle-quality graphics setting. The per-block routing
    /// (`CRenderBlockParticle::GetRenderDetails`) selects the pass index from this flag ORed with
    /// [`m_ForceLowResRendering`](RenderBlockTypeParticle::m_ForceLowResRendering).
    pub m_LowResRendering: bool,
    /// Forces every particle render block onto the low-resolution particle pass regardless of the
    /// per-effect opt-in or the distance threshold, ORed with
    /// [`m_LowResRendering`](RenderBlockTypeParticle::m_LowResRendering).
    pub m_ForceLowResRendering: bool,
    _field_a87: [u8; 1],
}
fn _RenderBlockTypeParticle_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xA88], RenderBlockTypeParticle>([0u8; 0xA88]);
    }
    unreachable!()
}
impl RenderBlockTypeParticle {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418086696usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl RenderBlockTypeParticle {}
impl std::convert::AsRef<RenderBlockTypeParticle> for RenderBlockTypeParticle {
    fn as_ref(&self) -> &RenderBlockTypeParticle {
        self
    }
}
impl std::convert::AsMut<RenderBlockTypeParticle> for RenderBlockTypeParticle {
    fn as_mut(&mut self) -> &mut RenderBlockTypeParticle {
        self
    }
}
#[repr(C, align(8))]
/// The terrain render block *type* (the `CRenderBlockTerrain::CRenderBlockTypeTerrain` singleton).
/// Its `SetupConstantBuffers` uploads the per-LOD-slot hull/domain tessellation constant buffer —
/// which bakes the dispatch's
/// [`RenderContext::m_OffsetViewProjection`](graphics_engine::graphics_engine::RenderContext::m_OffsetViewProjection),
/// camera position, and tessellation factors — into `m_HDTypeConstants[slot]` (22 constant-buffer
/// handles at `0x60`), caching it per slot keyed on the frame the upload was made for.
pub struct RenderBlockTypeTerrain {
    _field_0: [u8; 272],
    /// Per-LOD-slot cache stamp: the
    /// [`RenderContext::m_RenderFrameNo`](graphics_engine::graphics_engine::RenderContext::m_RenderFrameNo)
    /// of the frame whose tessellation constants were last uploaded into that slot's constant buffer.
    /// `SetupConstantBuffers` re-uploads a slot only when the current frame's stamp differs, so the
    /// baked view-projection is written once per frame and reused for every draw of that slot within
    /// the frame.
    pub m_WasCBApplied: [u32; 22],
}
fn _RenderBlockTypeTerrain_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x168], RenderBlockTypeTerrain>([0u8; 0x168]);
    }
    unreachable!()
}
impl RenderBlockTypeTerrain {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417914920usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl RenderBlockTypeTerrain {}
impl std::convert::AsRef<RenderBlockTypeTerrain> for RenderBlockTypeTerrain {
    fn as_ref(&self) -> &RenderBlockTypeTerrain {
        self
    }
}
impl std::convert::AsMut<RenderBlockTypeTerrain> for RenderBlockTypeTerrain {
    fn as_mut(&mut self) -> &mut RenderBlockTypeTerrain {
        self
    }
}
#[repr(C, align(8))]
/// The volumetric-patch terrain render block *type* (the
/// `NGraphicsEngine::CRenderBlockTerrainPatch::CRenderBlockTypeTerrainPatch` singleton): the tessellated
/// cliff/overhang variant of [`RenderBlockTypeTerrain`], with the same per-slot constant-buffer caching.
pub struct RenderBlockTypeTerrainPatch {
    _field_0: [u8; 288],
    /// Per-LOD-slot cache stamp; see [`RenderBlockTypeTerrain::m_WasCBApplied`]. The constant-buffer
    /// handle array (`m_HDTypeConstants[22]`) sits at `0x70` for this variant, so the stamp array
    /// follows at `0x120`.
    pub m_WasCBApplied: [u32; 22],
}
fn _RenderBlockTypeTerrainPatch_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x178], RenderBlockTypeTerrainPatch>([0u8; 0x178]);
    }
    unreachable!()
}
impl RenderBlockTypeTerrainPatch {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417914936usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl RenderBlockTypeTerrainPatch {}
impl std::convert::AsRef<RenderBlockTypeTerrainPatch> for RenderBlockTypeTerrainPatch {
    fn as_ref(&self) -> &RenderBlockTypeTerrainPatch {
        self
    }
}
impl std::convert::AsMut<RenderBlockTypeTerrainPatch> for RenderBlockTypeTerrainPatch {
    fn as_mut(&mut self) -> &mut RenderBlockTypeTerrainPatch {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(8))]
/// A skinned draw batch within a character render block. The vertex data references palette slots;
/// `BatchToSkeletonLookup` maps each slot to its skeleton bone index when the palette is built
/// (`SetMatrixPalette`), so the batch's lookup table enumerates exactly the bones its geometry is
/// weighted to.
pub struct SkinBatch {
    pub BatchToSkeletonLookup: *mut i16,
    pub BatchSize: i32,
    /// The batch's index count (indices, not triangles; `DrawBatches` divides by 3).
    pub Size: i32,
    /// The batch's start offset in the block's index buffer.
    pub Offset: i32,
    _field_14: [u8; 4],
}
fn _SkinBatch_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], SkinBatch>([0u8; 0x18]);
    }
    unreachable!()
}
impl SkinBatch {}
impl std::convert::AsRef<SkinBatch> for SkinBatch {
    fn as_ref(&self) -> &SkinBatch {
        self
    }
}
impl std::convert::AsMut<SkinBatch> for SkinBatch {
    fn as_mut(&mut self) -> &mut SkinBatch {
        self
    }
}
