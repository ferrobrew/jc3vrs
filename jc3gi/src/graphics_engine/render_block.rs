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
/// The tree-trunk/branch render block (`NGraphicsEngine::CRenderBlockBark`, registered name
/// `"VegetationBark"`): the solid woody geometry of trees, as distinct from the leaf cards. Forward-lit
/// vegetation.
pub struct RenderBlockBark {}
impl RenderBlockBark {
    pub const Draw_ADDRESS: usize = 0x140136F90;
    /// Issues the color-pass geometry. The clip transform is a CPU-baked world-view-projection staged
    /// on vertex `cb1` registers 0..3 via `SetVertexProgramConstants` before any draw-kind routing:
    /// `cb1[0..3] = M_model_camera_relative · m_OffsetViewProjection` for the non-instanced case, or
    /// `m_OffsetViewProjection` verbatim for the instanced/billboard cases (the per-instance model
    /// matrix is then applied in-shader). The global `m_VPGlobals` is bound at `cb0` for wind/time
    /// globals only, not the view-projection. One of three draw kinds is selected from the instance-data
    /// pointer in [`CRBIInfo`](RBIInfo): a non-instanced `DrawIndexed`, a CPU-instanced
    /// `DrawIndexedInstanced` (per-instance stream at slot 2), or a GPU-indirect `DrawIndexedNoMutex`
    /// whose instance count lives in the type's `m_InstDrawParams` buffer. `m_RenderStatus` bits select
    /// bounce/GI, billboard, and geometry-program permutations.
    pub unsafe fn Draw(
        &self,
        render_context: *mut crate::graphics_engine::graphics_engine::RenderContext,
        info: *const crate::graphics_engine::render_block::RBIInfo,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                render_context: *mut crate::graphics_engine::graphics_engine::RenderContext,
                info: *const crate::graphics_engine::render_block::RBIInfo,
            ) = ::std::mem::transmute(Self::Draw_ADDRESS);
            f(self as *const Self as _, render_context, info)
        }
    }
    pub const DrawZ_ADDRESS: usize = 0x140136A90;
    /// Issues the depth-prepass and depth-and-velocity geometry. Same cb1-baked transform as
    /// [`Draw`](RenderBlockBark::Draw); the velocity pass additionally bakes the previous frame's
    /// world-view-projection into `cb1` registers 5..8 from `m_PreviousOffsetViewProjection`.
    pub unsafe fn DrawZ(
        &self,
        render_context: *mut crate::graphics_engine::graphics_engine::RenderContext,
        info: *const crate::graphics_engine::render_block::RBIInfo,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                render_context: *mut crate::graphics_engine::graphics_engine::RenderContext,
                info: *const crate::graphics_engine::render_block::RBIInfo,
            ) = ::std::mem::transmute(Self::DrawZ_ADDRESS);
            f(self as *const Self as _, render_context, info)
        }
    }
}
impl std::convert::AsRef<RenderBlockBark> for RenderBlockBark {
    fn as_ref(&self) -> &RenderBlockBark {
        self
    }
}
impl std::convert::AsMut<RenderBlockBark> for RenderBlockBark {
    fn as_mut(&mut self) -> &mut RenderBlockBark {
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
/// The grass/foliage render block (`NGraphicsEngine::CRenderBlockFoliage`, registered name
/// `"VegetationFoliage"`): forward-lit ground cover and small plants, drawn in `RP_VEGETATION_OPAQUE`.
/// The bulk is grass drawn GPU-indirect.
pub struct RenderBlockFoliage {}
impl RenderBlockFoliage {
    pub const Draw_ADDRESS: usize = 0x14012DDA0;
    /// Issues the color-pass geometry. The clip transform is staged per draw on vertex `cb2`: register 0
    /// the camera-relative world matrix (from [`CRBIInfo`](RBIInfo)'s matrix, translation minus
    /// `m_CameraPosition`), registers 4..7 a per-draw copy of `m_OffsetViewProjection` (byte-identical to
    /// the global `cb0[29..32]`), so the vertex shader composes `clip = world · OffsetVP` from `cb2`
    /// rather than reading `cb0`. `SetupConstantBuffers` binds `cb0 = m_VPGlobals` but only for globals.
    /// One of three draw kinds is selected from the instance-data flags: a CPU-instanced
    /// `DrawIndexedInstancedNoMutex` (instance count a CPU `u16`), the dominant grass path
    /// `DrawIndexedInstancedIndirect` (instance count in the type's GPU-only `m_InstDrawParams` args
    /// buffer, populated by the vegetation draw-indirect compute pass), or a non-instanced
    /// `DrawIndexedNoMutex`. The pass is forward-lit: the type's `Setup` binds the clustered-lighting
    /// constant buffer, the light-cluster index texture, GI, reflection, and sun-shadow-cascade
    /// resources to the fragment stage.
    pub unsafe fn Draw(
        &self,
        render_context: *mut crate::graphics_engine::graphics_engine::RenderContext,
        info: *const crate::graphics_engine::render_block::RBIInfo,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                render_context: *mut crate::graphics_engine::graphics_engine::RenderContext,
                info: *const crate::graphics_engine::render_block::RBIInfo,
            ) = ::std::mem::transmute(Self::Draw_ADDRESS);
            f(self as *const Self as _, render_context, info)
        }
    }
}
impl std::convert::AsRef<RenderBlockFoliage> for RenderBlockFoliage {
    fn as_ref(&self) -> &RenderBlockFoliage {
        self
    }
}
impl std::convert::AsMut<RenderBlockFoliage> for RenderBlockFoliage {
    fn as_mut(&mut self) -> &mut RenderBlockFoliage {
        self
    }
}
#[repr(C, align(8))]
/// The occluder render block (`NGraphicsEngine::CRenderBlockOccluder`, registered name `"Occluder"`): a
/// unit-cube depth proxy scaled per scene occluder, injected once per frame into `RP_Z_OCCLUDERS`
/// (pass 47) to prime the main camera depth buffer so later Z and G-buffer passes early-Z reject
/// occluded geometry. Depth-only (null fragment program). The CPU software-occlusion system consumes
/// the same occluder data independently and is not fed by this GPU depth pass.
pub struct RenderBlockOccluder {}
impl RenderBlockOccluder {
    pub const DrawZ_ADDRESS: usize = 0x14017DFA0;
    /// Issues the occluder-box depth geometry. The non-instanced path bakes
    /// `WVP = (world - camera_offset) · m_OffsetViewProjection` (via
    /// `CRenderBlock::CalculateOffsetWorldViewProjectionMatrix`, `0x140136070`) into vertex `cb1`
    /// registers 0..3, register 4 a depth bias, then `DrawIndexed` per box. The instanced path
    /// (`gfx.occluders.use_instancing`) instead reads the global `m_VPGlobals` view-projection at `cb0`
    /// with per-instance world rows from a vertex stream and issues one `DrawIndexedInstanced`.
    /// `Draw`/`Setup` tail-call this and its `SetupZ`.
    pub unsafe fn DrawZ(
        &self,
        render_context: *mut crate::graphics_engine::graphics_engine::RenderContext,
        info: *const crate::graphics_engine::render_block::RBIInfo,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                render_context: *mut crate::graphics_engine::graphics_engine::RenderContext,
                info: *const crate::graphics_engine::render_block::RBIInfo,
            ) = ::std::mem::transmute(Self::DrawZ_ADDRESS);
            f(self as *const Self as _, render_context, info)
        }
    }
}
impl std::convert::AsRef<RenderBlockOccluder> for RenderBlockOccluder {
    fn as_ref(&self) -> &RenderBlockOccluder {
        self
    }
}
impl std::convert::AsMut<RenderBlockOccluder> for RenderBlockOccluder {
    fn as_mut(&mut self) -> &mut RenderBlockOccluder {
        self
    }
}
#[repr(C, align(8))]
/// A base VolumetricTerrain render block instance (`CRenderBlockTerrain`): one per terrain tile/sector.
pub struct RenderBlockTerrain {}
impl RenderBlockTerrain {
    pub const HullClipType_ADDRESS: usize = 0x14032B450;
    /// Returns the hull-clip type (`ETerrainHullClipType`: 0, 1, or 2) that selects the hull program
    /// `Setup`, `SetupZ`, and `Draw` bind (`m_HullProgram[clip + 4*scheme]`). Type 1 is the LOD clip:
    /// its hull samples the global terrain mask (the `VisibilityMask` texture bound from
    /// `SGlobalRBTerrainContext`) at the patch's four corners and zeroes the tessellation factors —
    /// discarding the patch — when every corner reads below 0.05, i.e. when a finer-LOD tile has fully
    /// faded in over that footprint. Type 2 is the water clip: patches below the cached water level
    /// are discarded when the camera is above water. Type 0 clips nothing. Tiles above the base LOD
    /// (`m_PatchLOD > 9`) always resolve to type 1; base-LOD tiles resolve to type 2 when the tile is
    /// at the context's high-detail LOD and the camera is above water (or the render context has
    /// render-status bit 3 set), and to type 0 otherwise — except that when
    /// `CWaterPatchManager::m_EnableWaterRenderPass` is clear, base-LOD tiles also resolve to type 1.
    /// The same selection runs in every pass (depth prepass and color alike).
    pub unsafe fn HullClipType(
        &self,
        render_context: *mut crate::graphics_engine::graphics_engine::RenderContext,
    ) -> i64 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                render_context: *mut crate::graphics_engine::graphics_engine::RenderContext,
            ) -> i64 = ::std::mem::transmute(Self::HullClipType_ADDRESS);
            f(self as *const Self as _, render_context)
        }
    }
}
impl std::convert::AsRef<RenderBlockTerrain> for RenderBlockTerrain {
    fn as_ref(&self) -> &RenderBlockTerrain {
        self
    }
}
impl std::convert::AsMut<RenderBlockTerrain> for RenderBlockTerrain {
    fn as_mut(&mut self) -> &mut RenderBlockTerrain {
        self
    }
}
#[repr(C, align(8))]
/// The terrain detail render block (`NGraphicsEngine::CTerrainRenderBlockDetail`, engine name
/// "TerrainDetail"): the procedurally-generated detail rock skin — cliff walls, cave ceilings, and
/// near-field rock detail scattered on the base terrain. Its vertices/indices/texels are produced
/// each frame by the compute pipeline in the sibling terrain-setup block into shared structured
/// buffers, and drawn GPU-indirect via a single `DrawIndexedInstancedIndirect` (the colour pass runs
/// plain vertex+fragment, no tessellation). The vertex shader reads a patch-local vertex and
/// transforms it by a CPU-baked `cb1` (vertex slot 1) whose rows are `T_patch · m_OffsetViewProjection`,
/// where `T_patch` translates by the patch origin expressed relative to the camera; the resulting
/// clip is the standard `m_OffsetViewProjection · (world - m_CameraPosition)`. `cb1` is staged by the
/// block's per-patch `Setup` (via `SetVertexProgramConstants` on vertex slot 1) immediately before
/// each `Draw`.
pub struct RenderBlockTerrainDetail {
    _field_0: [u8; 48],
    /// The patch origin's world X. The vertices are patch-local in X relative to this; the baked `cb1`
    /// folds `(m_WorldPatchX - m_CameraPosition.x)` into its translation row.
    pub m_WorldPatchX: f32,
    /// The patch origin's world Z (see `m_WorldPatchX`).
    pub m_WorldPatchZ: f32,
    _field_38: [u8; 16],
}
fn _RenderBlockTerrainDetail_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x48], RenderBlockTerrainDetail>([0u8; 0x48]);
    }
    unreachable!()
}
impl RenderBlockTerrainDetail {
    pub const Draw_ADDRESS: usize = 0x140326050;
    /// Issues the colour-pass draw: a single `DrawIndexedInstancedIndirect` over the compute-generated
    /// detail geometry, using the `cb1` transform staged by `Setup`.
    pub unsafe fn Draw(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
        info: *const crate::graphics_engine::render_block::RBIInfo,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
                info: *const crate::graphics_engine::render_block::RBIInfo,
            ) = ::std::mem::transmute(Self::Draw_ADDRESS);
            f(self as *mut Self as _, ctx, info)
        }
    }
}
impl std::convert::AsRef<RenderBlockTerrainDetail> for RenderBlockTerrainDetail {
    fn as_ref(&self) -> &RenderBlockTerrainDetail {
        self
    }
}
impl std::convert::AsMut<RenderBlockTerrainDetail> for RenderBlockTerrainDetail {
    fn as_mut(&mut self) -> &mut RenderBlockTerrainDetail {
        self
    }
}
#[repr(C, align(8))]
/// One volumetric-terrain patch instance (`NGraphicsEngine::CRenderBlockTerrainPatch`): the
/// per-patch render block the terrain patch system enqueues onto the terrain basemesh passes, one
/// per quadtree patch. Partial: only the placement and sort fields are mapped.
pub struct RenderBlockTerrainPatch {
    _field_0: [u8; 200],
    /// The patch's world-space placement origin. Together with
    /// [`m_Size`](RenderBlockTerrainPatch::m_Size) it spans the patch's footprint; the patch's
    /// 512 m tile indices ([`m_TileX`](RenderBlockTerrainPatch::m_TileX)) derive from the same
    /// placement.
    pub m_Position: crate::types::math::Vector3,
    /// The patch's world-space side length (quadtree level dependent).
    pub m_Size: f32,
    /// The patch's quadtree LOD level.
    pub m_PatchLOD: u16,
    _field_da: [u8; 6],
    /// The patch's 512 m world tile X index (offset by half the 32,768 m world).
    pub m_TileX: i16,
    /// The patch's 512 m world tile Z index.
    pub m_TileZ: i16,
    _field_e4: [u8; 12],
    /// The block's sort identifier, rebuilt per frame by
    /// [`UpdateSortID`](RenderBlockTerrainPatch::UpdateSortID): bits 32..47 carry the squared
    /// camera tile distance (in 512 m tiles), bits 61+ a tessellation/LOD class, and the low 32
    /// bits the block pointer. The block's `GetSortID` returns it verbatim, so terrain patches
    /// sort by tessellation class, then tile distance.
    pub m_SortID: u64,
}
fn _RenderBlockTerrainPatch_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xF8], RenderBlockTerrainPatch>([0u8; 0xF8]);
    }
    unreachable!()
}
impl RenderBlockTerrainPatch {
    pub const UpdateSortID_ADDRESS: usize = 0x1410CA030;
    /// Rebuilds [`m_SortID`](RenderBlockTerrainPatch::m_SortID) from the camera translation:
    /// recomputes the camera-relative tile deltas and packs the squared tile distance and
    /// tessellation class. Called by the terrain patch system's per-frame update.
    pub unsafe fn UpdateSortID(
        &mut self,
        camera_translation: *const crate::types::math::Vector3,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                camera_translation: *const crate::types::math::Vector3,
            ) = ::std::mem::transmute(Self::UpdateSortID_ADDRESS);
            f(self as *mut Self as _, camera_translation)
        }
    }
    pub const Draw_ADDRESS: usize = 0x14032E540;
    /// Issues the patch's geometry for the active render pass, keyed on
    /// [`m_ActiveRenderPass`](graphics_engine::graphics_engine::RenderContext::m_ActiveRenderPass):
    ///
    /// - Passes **56 and 57** (the near tessellating passes) draw GPU-indirect via
    ///   `DrawIndexedInstancedIndirectNoMutex`: the per-patch instance count comes from the terrain
    ///   patch system's GPU compute output, so no instance count is known CPU-side. Each stages a
    ///   one-`float4` control constant on vertex slot 3 (`{ m_DetailPatchIndex, mode, 0, 0 }`, `mode` = 2
    ///   for 56, 3 for 57) and binds the shared global detail index/vertex texture buffers before the
    ///   indirect draw.
    /// - Passes **58 and 60** draw the tail index range (`[m_SplitIndex, m_IndexCount)`) with a plain
    ///   `DrawIndexed`; passes **59 and 61** draw the head range (`[0, m_SplitIndex)`). The split
    ///   partitions the patch's index buffer between the two families.
    /// - Passes **14** and **38..=40** draw the full index range with a plain `DrawIndexed`.
    /// - Any other pass returns without drawing.
    ///
    /// The vertex transform for every path is the global view-projection: the type-level
    /// `SetupConstantBuffers` binds `m_VPGlobals` at vertex `cb0` (the rows carrying
    /// [`m_OffsetViewProjection`](graphics_engine::graphics_engine::RenderContext::m_OffsetViewProjection)),
    /// and the per-patch streamed constant buffer at vertex `cb1`; the tessellating passes additionally
    /// carry `m_OffsetViewProjection` in the hull/domain constants baked once per frame.
    pub unsafe fn Draw(
        &self,
        render_context: *mut crate::graphics_engine::graphics_engine::RenderContext,
        info: *const crate::graphics_engine::render_block::RBIInfo,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                render_context: *mut crate::graphics_engine::graphics_engine::RenderContext,
                info: *const crate::graphics_engine::render_block::RBIInfo,
            ) = ::std::mem::transmute(Self::Draw_ADDRESS);
            f(self as *const Self as _, render_context, info)
        }
    }
}
impl std::convert::AsRef<RenderBlockTerrainPatch> for RenderBlockTerrainPatch {
    fn as_ref(&self) -> &RenderBlockTerrainPatch {
        self
    }
}
impl std::convert::AsMut<RenderBlockTerrainPatch> for RenderBlockTerrainPatch {
    fn as_mut(&mut self) -> &mut RenderBlockTerrainPatch {
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
    _field_0: [u8; 76],
    /// The debug visualization mode. When `<= 0`, `Setup` selects the normal shading fragment
    /// programs; when `> 0`, it selects the debug fragment program at index `m_DebugMode + 13`
    /// instead (LOD colours, tessellation, and similar overlays; the program array holds 21
    /// entries, so 1..=7 are valid).
    pub m_DebugMode: i32,
    _field_50: [u8; 192],
    /// Per-LOD-slot cache stamp: the
    /// [`RenderContext::m_RenderFrameNo`](graphics_engine::graphics_engine::RenderContext::m_RenderFrameNo)
    /// of the frame whose tessellation constants were last uploaded into that slot's constant buffer.
    /// `SetupConstantBuffers` re-uploads a slot only when the current frame's stamp differs, so the
    /// baked view-projection is written once per frame and reused for every draw of that slot within
    /// the frame.
    pub m_WasCBApplied: [u32; 22],
    _field_168: [u8; 803],
    /// When set, `Draw` returns before issuing any draw call, suppressing every base-terrain block.
    pub m_NoDraw: bool,
    _field_48c: [u8; 8],
    /// Enables back-patch culling in the color pass. When set, `SetupConstantBuffers` bakes a cull flag
    /// (gated on the color-pass render-status bit) into the hull/domain constant buffer alongside the
    /// normalized forward vector of the camera manager's render camera and the
    /// [`m_BackPatchCullThreshold`](RenderBlockTypeTerrain::m_BackPatchCullThreshold), so the hull
    /// shader discards patches whose facing is beyond the threshold relative to that view direction.
    pub m_EnableBackPatchCulling: bool,
    /// Enables frustum patch culling in the color pass. When set (and the active pass is not a shadow
    /// cascade or one of the passes 63..=64), `SetupConstantBuffers` bakes a cull flag into the
    /// hull/domain constant buffer so the hull shader discards patches outside the baked
    /// [`m_OffsetViewProjection`](graphics_engine::graphics_engine::RenderContext::m_OffsetViewProjection)
    /// frustum.
    pub m_EnableFrustumPatchCulling: bool,
    /// Debug flag: when set, `Setup` leaves the cull face at `NONE` in the color pass (rather than
    /// `BACK`), so patches the hull shader would otherwise cull are still rasterized — a visualization
    /// of what patch culling removes.
    pub m_ShowDebugCulling: bool,
    /// Enables the per-detail cull term baked into the hull/domain constant buffer by
    /// `SetupConstantBuffers`.
    pub m_EnableCullByDetail: bool,
    /// The inner tessellation factor baked into the hull/domain constant buffer.
    pub m_TessellationFactorInner: f32,
    /// The edge tessellation factor baked into the hull/domain constant buffer. The hull shader scales
    /// each patch's edge tessellation from this by the patch's projected screen-space size; when the
    /// resulting factor falls to zero or below, the tessellator discards the patch.
    pub m_TessellationFactorEdge: f32,
    /// The minimum screen-space spacing target for tessellation, baked as its reciprocal into the
    /// hull/domain constant buffer. Smaller values raise the tessellation factor a given projected
    /// patch size resolves to.
    pub m_TessellationFactorMinSpacing: f32,
    /// The sphere (curvature) tessellation factor baked into the hull/domain constant buffer.
    pub m_TessellationFactorSphere: f32,
    /// The normal-difference tessellation factor baked into the hull/domain constant buffer.
    pub m_TessellationFactorNormalDiff: f32,
    _field_4ac: [u8; 16],
    /// The facing threshold for
    /// [`m_EnableBackPatchCulling`](RenderBlockTypeTerrain::m_EnableBackPatchCulling), baked into the
    /// hull/domain constant buffer. A patch is culled when its facing relative to the render camera's
    /// forward vector falls beyond this value.
    pub m_BackPatchCullThreshold: f32,
}
fn _RenderBlockTypeTerrain_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4C0], RenderBlockTypeTerrain>([0u8; 0x4C0]);
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
    _field_0: [u8; 76],
    /// The debug visualization mode. When `<= 0`, `Setup` selects the normal shading fragment
    /// programs; when `> 0`, it selects the debug fragment program at index `m_DebugMode + 60` instead
    /// (LOD colours, tessellation, and similar overlays).
    pub m_DebugMode: i32,
    _field_50: [u8; 208],
    /// Per-LOD-slot cache stamp; see [`RenderBlockTypeTerrain::m_WasCBApplied`]. The constant-buffer
    /// handle array (`m_HDTypeConstants[22]`) sits at `0x70` for this variant, so the stamp array
    /// follows at `0x120`.
    pub m_WasCBApplied: [u32; 22],
    _field_178: [u8; 264],
    /// The hull-program holders, indexed by clip type (each holder is two pointers; the first is
    /// what `Setup` passes to `SetHullProgram`). The type's `Setup` inlines the clip selection: the
    /// near passes (56..=57) bind index 0 (no clipping); every other tessellating pass (58, 60)
    /// binds index 2 — the LOD clip, whose hull discards patches that a finer-LOD tile's global
    /// mask footprint covers. Index 1 is the disabled-clip variant the (compiled-out) debug flag
    /// would select, and index 3 the detail-clip variant.
    pub m_HullProgramHolders: [u64; 8],
    _field_2c0: [u8; 393],
    /// When set, `Setup` binds the material-inspection fragment program instead of the shading one,
    /// overriding the debug-mode and tint selection.
    pub m_ShowMaterial: bool,
    _field_44a: [u8; 1],
    /// When set, the render block's draw is suppressed.
    pub m_NoDraw: bool,
    _field_44c: [u8; 1],
    /// Enables back-patch culling in the color pass. When set, `SetupConstantBuffers` bakes a cull flag
    /// (gated on the color-pass render-status bit) into the hull/domain constant buffer alongside the
    /// normalized forward vector of the camera manager's render camera and the
    /// [`m_BackPatchCullThreshold`](RenderBlockTypeTerrainPatch::m_BackPatchCullThreshold), so the hull
    /// shader discards patches whose facing is beyond the threshold relative to that view direction.
    pub m_EnableBackPatchCulling: bool,
    /// Enables frustum patch culling in the color pass. When set (and the active pass is not a shadow
    /// cascade or one of the passes 57..=60), `SetupConstantBuffers` bakes a cull flag into the
    /// hull/domain constant buffer so the hull shader discards patches outside the baked
    /// [`m_OffsetViewProjection`](graphics_engine::graphics_engine::RenderContext::m_OffsetViewProjection)
    /// frustum.
    pub m_EnableFrustumPatchCulling: bool,
    /// Debug flag: when set, `Setup` leaves the cull face at `NONE` in the color pass (rather than
    /// `BACK`), so patches the hull shader would otherwise cull are still rasterized — a visualization
    /// of what patch culling removes.
    pub m_ShowDebugCulling: bool,
    /// Enables the per-detail cull term baked into the hull/domain constant buffer by
    /// `SetupConstantBuffers`.
    pub m_EnableCullByDetail: bool,
    _field_451: [u8; 39],
    /// The facing threshold for
    /// [`m_EnableBackPatchCulling`](RenderBlockTypeTerrainPatch::m_EnableBackPatchCulling), baked into
    /// the hull/domain constant buffer. A patch is culled when its facing relative to the render
    /// camera's forward vector falls beyond this value.
    pub m_BackPatchCullThreshold: f32,
    _field_47c: [u8; 4],
}
fn _RenderBlockTypeTerrainPatch_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x480], RenderBlockTypeTerrainPatch>([0u8; 0x480]);
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
impl RenderBlockTypeTerrainPatch {
    pub const Setup_ADDRESS: usize = 0x14034BB30;
    /// The type-level per-pass setup for the color-family passes: binds the pass's vertex, hull,
    /// domain, and fragment programs (the depth-family passes route to `SetupZ` via the render
    /// status). The hull-clip selection is inlined here: passes 56..=57 bind
    /// [`m_HullProgramHolders`](RenderBlockTypeTerrainPatch::m_HullProgramHolders) index 0, other
    /// tessellating passes index 2.
    pub unsafe fn Setup(
        &mut self,
        render_context: *mut crate::graphics_engine::graphics_engine::RenderContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                render_context: *mut crate::graphics_engine::graphics_engine::RenderContext,
            ) = ::std::mem::transmute(Self::Setup_ADDRESS);
            f(self as *mut Self as _, render_context)
        }
    }
}
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
#[repr(C, align(8))]
/// The tree-impostor render block (`CTreeImpostorRB`): far-distance flat billboard cards that replace
/// full tree meshes at range.
pub struct TreeImpostorRB {}
impl TreeImpostorRB {
    pub const Draw_ADDRESS: usize = 0x14034F520;
    /// Issues the impostor cards as a single non-instanced `DrawIndexed` over a static quad index
    /// buffer (`6 * min(card_count, 0x1400)` indices); the vertex shader keys off
    /// `SV_VertexID` (`card = id >> 2`, `corner = id & 3`), pulling each card's world position, size, and
    /// atlas data from a texture buffer bound at vertex slot 0. The card orientation (facing) is computed
    /// in the vertex shader from the render camera carried in the engine's global per-view billboard
    /// constant buffer (the translation-bearing `m_ViewProjectionF`); the block stages only per-draw
    /// scalars on `cb1`. `m_RenderStatus & 6` selects the depth/shadow vertex- and fragment-program
    /// permutations. The vertex shader writes `SV_Position` directly, so this is not GPU-indirect.
    pub unsafe fn Draw(
        &self,
        render_context: *mut crate::graphics_engine::graphics_engine::RenderContext,
        info: *const crate::graphics_engine::render_block::RBIInfo,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                render_context: *mut crate::graphics_engine::graphics_engine::RenderContext,
                info: *const crate::graphics_engine::render_block::RBIInfo,
            ) = ::std::mem::transmute(Self::Draw_ADDRESS);
            f(self as *const Self as _, render_context, info)
        }
    }
}
impl std::convert::AsRef<TreeImpostorRB> for TreeImpostorRB {
    fn as_ref(&self) -> &TreeImpostorRB {
        self
    }
}
impl std::convert::AsMut<TreeImpostorRB> for TreeImpostorRB {
    fn as_mut(&mut self) -> &mut TreeImpostorRB {
        self
    }
}
