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
/// The WaveWorks water render block (`NGraphicsEngine::CNvWaterHighEndRenderBlock`): the ocean
/// surface at the higher water-quality settings, simulated and tessellated through NVIDIA WaveWorks
/// (`gfsdk_waveworks.win64.dll`) rather than through the engine's own patch grid.
///
/// Its block type registers itself as `"NvWaterHighEnd"` and owns the whole `NvWater*` shader family
/// — `NvWaterShader_lod0[_tess]`, `NvWaterShader_lod1[_tess]`, `NvWaterBelow`,
/// `NvWaterBelowShader_lod1`, `NvWaterLake`, and `NvWaterBox[_tess]` — selecting among them per draw
/// from the tessellation cvar and the block's above/below-surface flag. It also owns the two
/// constant buffers those permutations read: a 10-register buffer bound to **vertex and domain slot
/// 1** and a 15-register buffer bound to **fragment slot 1**.
pub struct NvWaterHighEndRenderBlock {}
impl NvWaterHighEndRenderBlock {
    pub const Setup_ADDRESS: usize = 0x140365040;
    /// Bakes the per-view WaveWorks matrices and stages both of the block type's constant buffers.
    ///
    /// The block keeps three 4x4 matrices of its own, all rebuilt here from the render context:
    /// - the WaveWorks **view** matrix, `Y/Z-swap · `[`RenderContext::m_View`](crate::graphics_engine::graphics_engine::RenderContext::m_View)
    ///   (WaveWorks works in a Z-up frame, the engine in a Y-up one);
    /// - the WaveWorks **projection** matrix, a verbatim copy of
    ///   [`RenderContext::m_ProjectionF`](crate::graphics_engine::graphics_engine::RenderContext::m_ProjectionF);
    /// - their product, the `g_ModelViewProjectionMatrix` the `NvWater*` vertex and domain shaders
    ///   write clip position from.
    ///
    /// It then maps the vertex/domain constant buffer write-discard and writes that product into
    /// **registers 0..3**, followed by the top-down-camera offset/scale, wind direction, character
    /// world position, time, and Gerstner tuning through byte offset 148; and maps the fragment
    /// constant buffer, writing the WaveWorks view matrix into its registers 0..3 followed by the
    /// water colour, scattering, fog, and foam tuning. Neither buffer is written anywhere else, and
    /// [`Draw`](crate::graphics_engine::render_block::NvWaterHighEndRenderBlock::Draw) restages nothing — it hands the same two block-held matrices straight to
    /// WaveWorks — so the whole family's view depends solely on the render context this call read.
    ///
    /// The body is selected by [`RenderContext::m_ActiveRenderPass`](crate::graphics_engine::graphics_engine::RenderContext::m_ActiveRenderPass):
    /// `PRE_RP_WATER_CS_PRE` (41) only refreshes the block's cached camera info, `PRE_RP_WATER_WAKES_PRE`
    /// (42) and `PRE_RP_WATER_FOAM_PRE` (43) only set the blend state and the wake/foam viewport
    /// offsets, and every other pass takes the matrix-and-constant-buffer path described above.
    ///
    /// `sort_id` and `previous_sort_id` come from the draw-list walk in
    /// [`RenderPass::DoDraw`](crate::graphics_engine::render_pass::RenderPass): the element's sort id, and
    /// `~0` on a block-type change. This override reads neither.
    pub unsafe fn Setup(
        &self,
        rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
        sort_id: u64,
        previous_sort_id: u64,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
                sort_id: u64,
                previous_sort_id: u64,
            ) = ::std::mem::transmute(Self::Setup_ADDRESS);
            f(self as *const Self as _, rc, sort_id, previous_sort_id)
        }
    }
    pub const Draw_ADDRESS: usize = 0x14037AE00;
    /// Draws the WaveWorks ocean: the simulation step, the quadtree surface, the water-box surfaces,
    /// and the far LOD skirt.
    ///
    /// On the same three-way pass split as [`Setup`](crate::graphics_engine::render_block::NvWaterHighEndRenderBlock::Setup), `PRE_RP_WATER_CS_PRE` (41) runs the
    /// compute foam sub-pass and `PRE_RP_WATER_FOAM_PRE` (43) the painted foam; every other pass takes
    /// the main body, which in order: advances the ocean simulation
    /// ([`WaveWorksSimulationStep`](crate::graphics_engine::render_block::WaveWorksSimulationStep)) unless the block's
    /// simulation-suppressed flag is set; binds the render context's render setup, the `NvWater*`
    /// program permutation for the current tessellation and above/below-surface state, and the water
    /// depth/stencil state; calls `GFSDK_WaveWorks_Simulation_SetRenderStateD3D11` and
    /// `GFSDK_WaveWorks_Quadtree_DrawD3D11` on the raw `ID3D11DeviceContext`, passing the two matrices
    /// [`Setup`](crate::graphics_engine::render_block::NvWaterHighEndRenderBlock::Setup) baked as the view and projection the quadtree culls and picks LODs
    /// against; draws the registered water-box surfaces through `NWater::DrawWaterBoxSurface`; and
    /// restores the D3D state WaveWorks changed with `GFSDK_WaveWorks_Savestate_RestoreD3D11`. If the
    /// block's far-LOD flag is set it then draws the far skirt quad as a plain [`DrawIndexed`](crate::graphics_engine::draw::DrawIndexed).
    ///
    /// The save and the restore of the WaveWorks savestate both happen inside this call, so no D3D
    /// state it captures outlives the call.
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
}
impl std::convert::AsRef<NvWaterHighEndRenderBlock> for NvWaterHighEndRenderBlock {
    fn as_ref(&self) -> &NvWaterHighEndRenderBlock {
        self
    }
}
impl std::convert::AsMut<NvWaterHighEndRenderBlock> for NvWaterHighEndRenderBlock {
    fn as_mut(&mut self) -> &mut NvWaterHighEndRenderBlock {
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
    /// The render blocks pass [`RenderContext::m_TransformIndex`](crate::graphics_engine::graphics_engine::RenderContext::m_TransformIndex)
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
/// position from depth via [`Matrix4::PerspectiveFovInverse`](crate::types::math::Matrix4) -- for the whole
/// screen, sky included -- and then ray-marches the sun shadow cascade and aerial perspective over
/// the reconstructed positions.
pub struct RenderBlockAtmosphericScattering {}
impl RenderBlockAtmosphericScattering {
    pub const Draw_ADDRESS: usize = 0x14036A820;
    /// Draws the atmospheric-scattering pass. `rc` is the per-view render context; `info` the
    /// instance info. Reconstructs view rays from depth via
    /// [`Matrix4::PerspectiveFovInverse`](crate::types::math::Matrix4) and samples the sun cascade.
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
    /// on vertex `cb1` registers 0..3 via [`SetVertexProgramConstants`](crate::graphics_engine::draw::SetVertexProgramConstants) before any draw-kind routing:
    /// `cb1[0..3] = M_model_camera_relative · m_OffsetViewProjection` for the non-instanced case, or
    /// `m_OffsetViewProjection` verbatim for the instanced/billboard cases (the per-instance model
    /// matrix is then applied in-shader). The global `m_VPGlobals` is bound at `cb0` for wind/time
    /// globals only, not the view-projection. One of three draw kinds is selected from the instance-data
    /// pointer in [`CRBIInfo`](crate::graphics_engine::render_block::RBIInfo): a non-instanced [`DrawIndexed`](crate::graphics_engine::draw::DrawIndexed), a CPU-instanced
    /// [`DrawIndexedInstanced`](crate::graphics_engine::draw::DrawIndexedInstanced) (per-instance stream at slot 2), or a GPU-indirect `DrawIndexedNoMutex`
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
    /// [`Draw`](crate::graphics_engine::render_block::RenderBlockBark::Draw); the velocity pass additionally bakes the previous frame's
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
/// The skinned character render block (the [`Character`](crate::character::character::Character) RBMDL block type). A character model is
/// composed of one block per material; the same block objects are drawn for every pass, branching
/// internally on [`RenderContext::m_RenderStatus`](crate::graphics_engine::graphics_engine::RenderContext::m_RenderStatus)
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
    /// ([`RenderContext::m_RenderStatus`](crate::graphics_engine::graphics_engine::RenderContext::m_RenderStatus) `& 6`)
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
/// variant of [`RenderBlockCharacter`](crate::graphics_engine::render_block::RenderBlockCharacter), with the same batch and pass structure.
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
    /// Draws the block for the current pass; see [`RenderBlockCharacter::Draw`](crate::graphics_engine::render_block::RenderBlockCharacter::Draw).
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
    /// See [`RenderBlockCharacter::SetMatrixPalette`](crate::graphics_engine::render_block::RenderBlockCharacter::SetMatrixPalette).
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
    pub const Draw_ADDRESS: usize = 0x14013E1E0;
    /// Selects between the block's two lighting paths on a single condition:
    /// [`DrawPassThrough`](crate::graphics_engine::render_block::RenderBlockDeferredLighting::DrawPassThrough) when
    /// [`RenderEngine::IsWireframeEnabled`](crate::graphics_engine::render_engine::RenderEngine) reports
    /// wireframe, [`DrawClustered`](crate::graphics_engine::render_block::RenderBlockDeferredLighting::DrawClustered) otherwise. Normal shaded rendering therefore
    /// always takes the clustered path.
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
    pub const DrawPassThrough_ADDRESS: usize = 0x14013CD00;
    /// The wireframe-only lighting path, reached from [`Draw`](crate::graphics_engine::render_block::RenderBlockDeferredLighting::Draw) exclusively when
    /// [`RenderEngine::IsWireframeEnabled`](crate::graphics_engine::render_engine::RenderEngine) is set, so
    /// it does not run during normal shaded rendering. Like the clustered path it recovers a
    /// clip-to-view basis via [`Matrix4::PerspectiveFovInverse`](crate::types::math::Matrix4).
    pub unsafe fn DrawPassThrough(
        &self,
        rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
            ) = ::std::mem::transmute(Self::DrawPassThrough_ADDRESS);
            f(self as *const Self as _, rc)
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
    /// the camera-relative world matrix (from [`CRBIInfo`](crate::graphics_engine::render_block::RBIInfo)'s matrix, translation minus
    /// `m_CameraPosition`), registers 4..7 a per-draw copy of `m_OffsetViewProjection` (byte-identical to
    /// the global `cb0[29..32]`), so the vertex shader composes `clip = world · OffsetVP` from `cb2`
    /// rather than reading `cb0`. `SetupConstantBuffers` binds `cb0 = m_VPGlobals` but only for globals.
    /// One of three draw kinds is selected from the instance-data flags: a CPU-instanced
    /// `DrawIndexedInstancedNoMutex` (instance count a CPU `u16`), the dominant grass path
    /// [`DrawIndexedInstancedIndirect`](crate::graphics_engine::draw::DrawIndexedInstancedIndirect) (instance count in the type's GPU-only `m_InstDrawParams` args
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
/// occluded geometry. Depth-only (null fragment program), and write-only: nothing reads the primed
/// depth back on the CPU. The CPU software-occlusion system consumes the same occluder source data
/// through its own path and is not fed by this GPU depth pass.
pub struct RenderBlockOccluder {}
impl RenderBlockOccluder {
    pub const DrawZ_ADDRESS: usize = 0x14017DFA0;
    /// Issues the occluder-box depth geometry. The non-instanced path bakes
    /// `WVP = (world - camera_offset) · m_OffsetViewProjection` (via
    /// `CRenderBlock::CalculateOffsetWorldViewProjectionMatrix`, `0x140136070`) into vertex `cb1`
    /// registers 0..3, register 4 a depth bias, then [`DrawIndexed`](crate::graphics_engine::draw::DrawIndexed) per box. The instanced path
    /// (`gfx.occluders.use_instancing`) instead reads the global `m_VPGlobals` view-projection at `cb0`
    /// with per-instance world rows from a vertex stream and issues one [`DrawIndexedInstanced`](crate::graphics_engine::draw::DrawIndexedInstanced).
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
/// The screen-space ambient-occlusion render block. Its `Draw` reconstructs view-space position from
/// depth via [`Matrix4::PerspectiveFovInverse`](crate::types::math::Matrix4) and carries a temporal history
/// across frames, indexed by a counter the block advances per invocation.
pub struct RenderBlockSSAO {}
impl RenderBlockSSAO {
    pub const Draw_ADDRESS: usize = 0x140190E80;
    /// Draws the ambient-occlusion pass, reconstructing from depth and accumulating into the
    /// temporal history.
    ///
    /// One call runs five phases, each binding its own render setup, and they do not share a
    /// resolution: the [`SSAOPass`](crate::graphics_engine::ssao::SSAOPass) sizes its depth targets at the
    /// render resolution scaled by `m_Resolution` and its occlusion and history targets at half that
    /// again, while the final composite goes to the full-resolution scene target.
    ///
    /// 1. A linear-depth pack into the current slot of the depth targets, followed by a whole-texture
    ///    mip generation over it (the targets carry five mip levels).
    /// 2. The occlusion generation into the first of the two occlusion targets, reconstructing
    ///    view-space position from the packed depth using the render context's own projection terms
    ///    and an inverse-transpose of its camera transform -- not through
    ///    [`Matrix4::PerspectiveFovInverse`](types::math::Matrix4).
    /// 3. A separable bilateral blur, when the pass's blur flag is set: two draws ping-ponging between
    ///    the two occlusion targets, so the blurred result ends up back in the first. Neither draw is
    ///    idempotent -- each reads and writes the same pair of textures.
    /// 4. The temporal resolve, when [`m_EnableTemporalFilter`](crate::graphics_engine::ssao::SSAOPass) is
    ///    set, into the current slot of the history targets, sampling the previous slot. This is the
    ///    only phase that calls [`Matrix4::PerspectiveFovInverse`](types::math::Matrix4), uploading the
    ///    composed clip-to-world basis as vertex constants 1..4 of constant buffer 2. Unless the pass
    ///    is on its first frame it is preceded by a second separable blur pair, over the previous
    ///    history slot.
    /// 5. The composite into the alpha channel of the scene target (colour mask 8, depth test on),
    ///    followed by the inlined history advance.
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
}
impl std::convert::AsRef<RenderBlockSSAO> for RenderBlockSSAO {
    fn as_ref(&self) -> &RenderBlockSSAO {
        self
    }
}
impl std::convert::AsMut<RenderBlockSSAO> for RenderBlockSSAO {
    fn as_mut(&mut self) -> &mut RenderBlockSSAO {
        self
    }
}
#[repr(C, align(8))]
/// The screen-space decal render block (`NGraphicsEngine::CRenderBlockSSDecal`): one projected decal
/// box, drawn with the permutations set up by [`RenderBlockTypeSSDecal`](crate::graphics_engine::render_block::RenderBlockTypeSSDecal).
pub struct RenderBlockSSDecal {}
impl RenderBlockSSDecal {
    pub const Draw_ADDRESS: usize = 0x14017FD40;
    /// Stages the decal's own constants and issues its box geometry as a single [`DrawIndexed`](crate::graphics_engine::draw::DrawIndexed).
    ///
    /// On the vertex side it bakes the instance's offset world-view-projection at slot 1 registers
    /// 0..3 and the decal's orthonormal box basis at registers 4..7. On the fragment side it stages
    /// the decal's colour, fade, and masking tuning at slot 1 registers 4..11, and at **register 12**
    /// the screen-space UV scale the render context carries — the factor that maps a `[0, 1]` viewport
    /// UV onto the used region of the shared scene depth texture, which the permutations multiply
    /// their reconstruction UV by before sampling it. It also sets the colour mask from the decal's
    /// own channel flags, so the draw cannot be reproduced by re-issuing the underlying
    /// [`DrawIndexed`](crate::graphics_engine::draw::DrawIndexed) alone.
    ///
    /// The depth→view-space basis this consumes is whatever
    /// [`RenderBlockTypeSSDecal::Setup`](crate::graphics_engine::render_block::RenderBlockTypeSSDecal::Setup) staged for the pass; nothing
    /// here restages it.
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
}
impl std::convert::AsRef<RenderBlockSSDecal> for RenderBlockSSDecal {
    fn as_ref(&self) -> &RenderBlockSSDecal {
        self
    }
}
impl std::convert::AsMut<RenderBlockSSDecal> for RenderBlockSSDecal {
    fn as_mut(&mut self) -> &mut RenderBlockSSDecal {
        self
    }
}
#[repr(C, align(8))]
/// The screen-space reflection render block. Its `Draw` reconstructs world position from depth via
/// [`Matrix4::PerspectiveFovInverse`](crate::types::math::Matrix4) and ray-marches a scene colour capture
/// taken earlier in the frame.
pub struct RenderBlockScreenSpaceReflection {}
impl RenderBlockScreenSpaceReflection {
    pub const Draw_ADDRESS: usize = 0x140191E10;
    /// Draws the screen-space reflection pass.
    ///
    /// One call runs, in order: a scene-colour capture draw; the ray-march draw, which is the only
    /// phase that calls [`Matrix4::PerspectiveFovInverse`](crate::types::math::Matrix4) (uploaded as vertex
    /// constants 1..4) and which writes the block's reflection-colour and reflection-alpha targets
    /// through a stencil test; and then a separable blur of those two targets, on one of two mutually
    /// exclusive paths selected by the block's compute-blur flag at `+0xA74`.
    ///
    /// The pixel-shader path is two fullscreen draws ping-ponging the colour/alpha pair against a
    /// second pair, so the blurred result lands back in the first pair. The compute path is two
    /// [`Dispatch`](crate::graphics_engine::draw::Dispatch) calls over the same two pairs, running the
    /// `screenspacereflectionblur` compute program (thread group 256x1x1):
    ///
    /// - The first blurs horizontally, dispatched `ceil(width / 256)` by `height` groups.
    /// - The second blurs vertically, dispatched `ceil(height / 256)` by `width` groups, with the
    ///   source and destination pairs exchanged so the result again ends up in the first pair.
    ///
    /// `width` and `height` are the block's own working resolution at `+0xA68` and `+0xA6C`. They are
    /// also uploaded as `x` and `y` of compute constant 0, whose `w` is the axis flag that tells the
    /// program which way to blur (1.0 horizontal, 0.0 vertical) by transposing its thread-id-to-texel
    /// mapping; there is no origin or offset term in either constant buffer, and the program addresses
    /// its textures directly by `SV_DispatchThreadID`, so the region it covers is always the whole
    /// texture from texel zero. `z` of that constant is the gloss threshold below which a texel is
    /// copied through unblurred.
    ///
    /// The blur kernel is a bilateral one over up to 13 taps, `-6..+6` texels along the blur axis,
    /// weighted down by the linear-depth difference to the centre texel and truncated to as few as 3
    /// taps as gloss rises. Its reach across the texture is therefore 6 texels, and the compute path
    /// stages that halo through group-shared memory (a 256-wide tile plus a 6-texel skirt on each
    /// side, clamped to the texture bounds).
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
}
impl std::convert::AsRef<RenderBlockScreenSpaceReflection>
for RenderBlockScreenSpaceReflection {
    fn as_ref(&self) -> &RenderBlockScreenSpaceReflection {
        self
    }
}
impl std::convert::AsMut<RenderBlockScreenSpaceReflection>
for RenderBlockScreenSpaceReflection {
    fn as_mut(&mut self) -> &mut RenderBlockScreenSpaceReflection {
        self
    }
}
#[repr(C, align(8))]
/// The screen-space subsurface-scattering render block for skin. Its `Draw` reconstructs from depth
/// via [`Matrix4::PerspectiveFovInverse`](crate::types::math::Matrix4) and blurs the lit skin in screen
/// space.
pub struct RenderBlockScreenSpaceSubSurfaceSkin {}
impl RenderBlockScreenSpaceSubSurfaceSkin {
    pub const Draw_ADDRESS: usize = 0x140192D60;
    /// Draws the subsurface-scattering pass, on one of two mutually exclusive paths selected by the
    /// block's own byte at `+0x18`: a single-draw path, and the full diffusion chain (a screen-space
    /// gather into the SSS targets, then six separable blur draws ping-ponging between them). Each
    /// path builds the depth-reconstruction basis with exactly one
    /// [`Matrix4::PerspectiveFovInverse`](crate::types::math::Matrix4) call, before its first render-setup
    /// bind, and uploads it as vertex constants 1..4.
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
}
impl std::convert::AsRef<RenderBlockScreenSpaceSubSurfaceSkin>
for RenderBlockScreenSpaceSubSurfaceSkin {
    fn as_ref(&self) -> &RenderBlockScreenSpaceSubSurfaceSkin {
        self
    }
}
impl std::convert::AsMut<RenderBlockScreenSpaceSubSurfaceSkin>
for RenderBlockScreenSpaceSubSurfaceSkin {
    fn as_mut(&mut self) -> &mut RenderBlockScreenSpaceSubSurfaceSkin {
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
/// buffers, and drawn GPU-indirect via a single [`DrawIndexedInstancedIndirect`](crate::graphics_engine::draw::DrawIndexedInstancedIndirect) (the colour pass runs
/// plain vertex+fragment, no tessellation). The vertex shader reads a patch-local vertex and
/// transforms it by a CPU-baked `cb1` (vertex slot 1) whose rows are `T_patch · m_OffsetViewProjection`,
/// where `T_patch` translates by the patch origin expressed relative to the camera; the resulting
/// clip is the standard `m_OffsetViewProjection · (world - m_CameraPosition)`. `cb1` is staged by the
/// block's per-patch `Setup` (via [`SetVertexProgramConstants`](crate::graphics_engine::draw::SetVertexProgramConstants) on vertex slot 1) immediately before
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
    /// Issues the colour-pass draw: a single [`DrawIndexedInstancedIndirect`](crate::graphics_engine::draw::DrawIndexedInstancedIndirect) over the compute-generated
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
    /// [`m_Size`](crate::graphics_engine::render_block::RenderBlockTerrainPatch::m_Size) it spans the patch's footprint; the patch's
    /// 512 m tile indices ([`m_TileX`](crate::graphics_engine::render_block::RenderBlockTerrainPatch::m_TileX)) derive from the same
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
    /// [`UpdateSortID`](crate::graphics_engine::render_block::RenderBlockTerrainPatch::UpdateSortID): bits 32..47 carry the squared
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
    /// Rebuilds [`m_SortID`](crate::graphics_engine::render_block::RenderBlockTerrainPatch::m_SortID) from the camera translation:
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
    /// [`m_ActiveRenderPass`](crate::graphics_engine::graphics_engine::RenderContext::m_ActiveRenderPass):
    ///
    /// - Passes **56 and 57** (the near tessellating passes) draw GPU-indirect via
    ///   `DrawIndexedInstancedIndirectNoMutex`: the per-patch instance count comes from the terrain
    ///   patch system's GPU compute output, so no instance count is known CPU-side. Each stages a
    ///   one-`float4` control constant on vertex slot 3 (`{ m_DetailPatchIndex, mode, 0, 0 }`, `mode` = 2
    ///   for 56, 3 for 57) and binds the shared global detail index/vertex texture buffers before the
    ///   indirect draw.
    /// - Passes **58 and 60** draw the tail index range (`[m_SplitIndex, m_IndexCount)`) with a plain
    ///   [`DrawIndexed`](crate::graphics_engine::draw::DrawIndexed); passes **59 and 61** draw the head range (`[0, m_SplitIndex)`). The split
    ///   partitions the patch's index buffer between the two families.
    /// - Passes **14** and **38..=40** draw the full index range with a plain [`DrawIndexed`](crate::graphics_engine::draw::DrawIndexed).
    /// - Any other pass returns without drawing.
    ///
    /// The vertex transform for every path is the global view-projection: the type-level
    /// `SetupConstantBuffers` binds `m_VPGlobals` at vertex `cb0` (the rows carrying
    /// [`m_OffsetViewProjection`](crate::graphics_engine::graphics_engine::RenderContext::m_OffsetViewProjection)),
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
/// The additive fog-volume render block *type* (the
/// `NGraphicsEngine::CRenderBlockAddFogVolume::CRenderBlockTypeAddFogVolume` singleton, type name
/// `"AddFogVolume"`): the shared vertex/fragment programs, unit vertex buffer, and sampler for the
/// additive fog-volume blocks that `CFogManager` draws through its own render passes.
///
/// **This type is never registered.** `CRenderBlockAddFogVolume::InitType` (`0x140_33D_E80`)
/// constructs it, stores it in this singleton, calls its `Create`, and returns — without calling
/// `CRenderBlockFactory::AddType`. Its only caller is the `CFogManager` constructor
/// (`0x140_358_870`), which does not register it either, so it never enters the global
/// [`RenderBlockTypeRegistry`](crate::graphics_engine::render_engine::RenderBlockTypeRegistry) and anything
/// that enumerates the registry to reach every type will silently miss it — even though
/// `CRenderPass::DoDraw` dispatches its `IsEnabled` through the vtable exactly as it does for a
/// registered type. This singleton is the only way to reach it. See
/// [`RenderBlockTypeParticle`](crate::graphics_engine::render_block::RenderBlockTypeParticle), which is unregistered in the same way.
pub struct RenderBlockTypeAddFogVolume {
    pub base: crate::graphics_engine::render_engine::RenderBlockTypeBase,
    _field_8: [u8; 88],
}
fn _RenderBlockTypeAddFogVolume_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x60], RenderBlockTypeAddFogVolume>([0u8; 0x60]);
    }
    unreachable!()
}
impl RenderBlockTypeAddFogVolume {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417825232usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl RenderBlockTypeAddFogVolume {
    pub fn vftable(
        &self,
    ) -> *const crate::graphics_engine::render_engine::RenderBlockTypeBaseVftable {
        self.base.vftable()
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
    /// calling [`Destroy`](crate::graphics_engine::render_block::RenderBlockTypeAddFogVolume::Destroy) and
    /// [`Create`](crate::graphics_engine::render_block::RenderBlockTypeAddFogVolume::Create) directly.
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
    /// replacing its [`IsEnabled`](crate::graphics_engine::render_block::RenderBlockTypeAddFogVolume::IsEnabled) vtable entry.
    pub unsafe fn Disable(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).Disable).read();
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<crate::graphics_engine::render_engine::RenderBlockTypeBase>
for RenderBlockTypeAddFogVolume {
    fn as_ref(&self) -> &crate::graphics_engine::render_engine::RenderBlockTypeBase {
        &self.base
    }
}
impl std::convert::AsMut<crate::graphics_engine::render_engine::RenderBlockTypeBase>
for RenderBlockTypeAddFogVolume {
    fn as_mut(
        &mut self,
    ) -> &mut crate::graphics_engine::render_engine::RenderBlockTypeBase {
        &mut self.base
    }
}
impl std::convert::AsRef<RenderBlockTypeAddFogVolume> for RenderBlockTypeAddFogVolume {
    fn as_ref(&self) -> &RenderBlockTypeAddFogVolume {
        self
    }
}
impl std::convert::AsMut<RenderBlockTypeAddFogVolume> for RenderBlockTypeAddFogVolume {
    fn as_mut(&mut self) -> &mut RenderBlockTypeAddFogVolume {
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
    /// [`ResizeTextures`](crate::graphics_engine::render_block::RenderBlockTypeFogVolume::ResizeTextures) call.
    pub m_HiResTextureWidth: u32,
    /// The full-resolution fog target height, in pixels; see
    /// [`m_HiResTextureWidth`](crate::graphics_engine::render_block::RenderBlockTypeFogVolume::m_HiResTextureWidth).
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
///
/// **This type is never registered.** `CRenderBlockParticle::InitType` (`0x140_4AD_2D0`) constructs
/// it, stores it in this singleton, calls its `Create`, and returns — without calling
/// `CRenderBlockFactory::AddType`, which every other render block's `InitType`/`RegisterType`
/// eventually does. So it does not appear in the global
/// [`RenderBlockTypeRegistry`](crate::graphics_engine::render_engine::RenderBlockTypeRegistry), and anything
/// that enumerates the registry to reach every type will silently miss it — even though
/// `CRenderPass::DoDraw` dispatches its `IsEnabled` through the vtable exactly as it does for a
/// registered type. This singleton is the only way to reach it.
///
/// Walking every construction site of `IRenderBlockType` in the image — the calls to its constructor
/// (`0x140_100_FC0`) plus the sites that inline it, which are exactly the writers of
/// [`RenderBlockTypeInstances`](crate::graphics_engine::render_engine::RenderBlockTypeInstances) — and
/// checking each against `CRenderBlockFactory::AddType` (`0x140_161_2F0`) shows two types skip
/// registration: this one and
/// [`RenderBlockTypeAddFogVolume`](crate::graphics_engine::render_block::RenderBlockTypeAddFogVolume).
pub struct RenderBlockTypeParticle {
    pub base: crate::graphics_engine::render_engine::RenderBlockTypeBase,
    _field_8: [u8; 2685],
    /// When set, a particle render block whose effect opts in and that falls below the low-resolution
    /// distance threshold routes its draw to the low-resolution particle pass (later composited back
    /// up by the low-res upsampling pass); when clear, that particle routes to the full-resolution
    /// transparent pass instead. Set from the particle-quality graphics setting. The per-block routing
    /// (`CRenderBlockParticle::GetRenderDetails`) selects the pass index from this flag ORed with
    /// [`m_ForceLowResRendering`](crate::graphics_engine::render_block::RenderBlockTypeParticle::m_ForceLowResRendering).
    pub m_LowResRendering: bool,
    /// Forces every particle render block onto the low-resolution particle pass regardless of the
    /// per-effect opt-in or the distance threshold, ORed with
    /// [`m_LowResRendering`](crate::graphics_engine::render_block::RenderBlockTypeParticle::m_LowResRendering).
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
impl RenderBlockTypeParticle {
    pub fn vftable(
        &self,
    ) -> *const crate::graphics_engine::render_engine::RenderBlockTypeBaseVftable {
        self.base.vftable()
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
    /// calling [`Destroy`](crate::graphics_engine::render_block::RenderBlockTypeAddFogVolume::Destroy) and
    /// [`Create`](crate::graphics_engine::render_block::RenderBlockTypeAddFogVolume::Create) directly.
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
    /// replacing its [`IsEnabled`](crate::graphics_engine::render_block::RenderBlockTypeAddFogVolume::IsEnabled) vtable entry.
    pub unsafe fn Disable(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).Disable).read();
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<crate::graphics_engine::render_engine::RenderBlockTypeBase>
for RenderBlockTypeParticle {
    fn as_ref(&self) -> &crate::graphics_engine::render_engine::RenderBlockTypeBase {
        &self.base
    }
}
impl std::convert::AsMut<crate::graphics_engine::render_engine::RenderBlockTypeBase>
for RenderBlockTypeParticle {
    fn as_mut(
        &mut self,
    ) -> &mut crate::graphics_engine::render_engine::RenderBlockTypeBase {
        &mut self.base
    }
}
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
/// The screen-space decal render block *type*
/// (`NGraphicsEngine::CRenderBlockSSDecal::CRenderBlockTypeSSDecal`): the per-pass setup shared by
/// the twelve `ssdecal*` fragment permutations, which project a decal's textures onto whatever the
/// scene depth buffer says is behind the decal's box volume.
pub struct RenderBlockTypeSSDecal {}
impl RenderBlockTypeSSDecal {
    pub const Setup_ADDRESS: usize = 0x140191C20;
    /// Binds the decal render state (alpha blend, depth test with depth writes off, back-face cull),
    /// the shared vertex program and vertex declaration, and stages the pass's **depth→view-space
    /// reconstruction basis on fragment slot 1, registers 0..3**.
    ///
    /// The basis is `Scaling(2, -2, 1)` with the translation row `(-1, 1, 0, 1)` — the viewport-UV to
    /// NDC map — post-multiplied (row-vector convention) by the inverse of
    /// [`RenderContext::m_View`](crate::graphics_engine::graphics_engine::RenderContext::m_View) with its
    /// translation row replaced by `(0, 0, 0, 1)`, times
    /// [`RenderContext::m_ProjectionF`](crate::graphics_engine::graphics_engine::RenderContext::m_ProjectionF).
    /// The fragment shaders feed it the interpolated projective screen coordinate divided by its `w`
    /// (a UV normalized over the *viewport*) together with the sampled scene depth, and divide the
    /// result by `w` to recover the camera-relative world position the decal box is tested against.
    ///
    /// It is staged once per pass, not per draw: [`RenderBlockSSDecal::Draw`](crate::graphics_engine::render_block::RenderBlockSSDecal::Draw) restages every other
    /// fragment constant the permutations read but never this one.
    ///
    /// The whole body is conditional on the render context's pass flags, so the depth-only and
    /// velocity passes stage nothing.
    pub unsafe fn Setup(
        &self,
        rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
            ) = ::std::mem::transmute(Self::Setup_ADDRESS);
            f(self as *const Self as _, rc)
        }
    }
    pub const SetupConstantBuffers_ADDRESS: usize = 0x14016D420;
    /// Binds the render context's global constant buffers to vertex and fragment slot 0 and declares
    /// the pass's instance-constant slots: vertex slot 1 at **8** `float4` rows (base row 0), fragment
    /// slot 1 at **13** rows (base row 0), and slots 2 and 3 unused on both stages.
    ///
    /// Those are exactly the rows the permutations declare: the shared `ssdecal` vertex program
    /// declares `cb1[7]` and the twelve fragment permutations declare `cb1[13]`. Because
    /// [`SetFragmentProgramConstantBufferSize`](crate::graphics_engine::draw::SetFragmentProgramConstantBufferSize) rounds a declared row count up to the next pool size
    /// class, the buffer actually bound to fragment slot 1 for this pass holds **16** rows and
    /// [`SetupRenderStates`](crate::graphics_engine::draw::SetupRenderStates) uploads all sixteen — the three rows past the declared thirteen carry
    /// whatever the shared staging array holds.
    pub unsafe fn SetupConstantBuffers(
        &self,
        rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
            ) = ::std::mem::transmute(Self::SetupConstantBuffers_ADDRESS);
            f(self as *const Self as _, rc)
        }
    }
}
impl std::convert::AsRef<RenderBlockTypeSSDecal> for RenderBlockTypeSSDecal {
    fn as_ref(&self) -> &RenderBlockTypeSSDecal {
        self
    }
}
impl std::convert::AsMut<RenderBlockTypeSSDecal> for RenderBlockTypeSSDecal {
    fn as_mut(&mut self) -> &mut RenderBlockTypeSSDecal {
        self
    }
}
#[repr(C, align(8))]
/// The terrain render block *type* (the `CRenderBlockTerrain::CRenderBlockTypeTerrain` singleton).
/// Its `SetupConstantBuffers` uploads the per-LOD-slot hull/domain tessellation constant buffer —
/// which bakes the dispatch's
/// [`RenderContext::m_OffsetViewProjection`](crate::graphics_engine::graphics_engine::RenderContext::m_OffsetViewProjection),
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
    /// [`RenderContext::m_RenderFrameNo`](crate::graphics_engine::graphics_engine::RenderContext::m_RenderFrameNo)
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
    /// [`m_BackPatchCullThreshold`](crate::graphics_engine::render_block::RenderBlockTypeTerrain::m_BackPatchCullThreshold), so the hull
    /// shader discards patches whose facing is beyond the threshold relative to that view direction.
    pub m_EnableBackPatchCulling: bool,
    /// Enables frustum patch culling in the color pass. When set (and the active pass is not a shadow
    /// cascade or one of the passes 63..=64), `SetupConstantBuffers` bakes a cull flag into the
    /// hull/domain constant buffer so the hull shader discards patches outside the baked
    /// [`m_OffsetViewProjection`](crate::graphics_engine::graphics_engine::RenderContext::m_OffsetViewProjection)
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
    /// [`m_EnableBackPatchCulling`](crate::graphics_engine::render_block::RenderBlockTypeTerrain::m_EnableBackPatchCulling), baked into the
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
/// cliff/overhang variant of [`RenderBlockTypeTerrain`](crate::graphics_engine::render_block::RenderBlockTypeTerrain), with the same per-slot constant-buffer caching.
pub struct RenderBlockTypeTerrainPatch {
    _field_0: [u8; 76],
    /// The debug visualization mode. When `<= 0`, `Setup` selects the normal shading fragment
    /// programs; when `> 0`, it selects the debug fragment program at index `m_DebugMode + 60` instead
    /// (LOD colours, tessellation, and similar overlays).
    pub m_DebugMode: i32,
    _field_50: [u8; 208],
    /// Per-LOD-slot cache stamp; see [`RenderBlockTypeTerrain::m_WasCBApplied`](crate::graphics_engine::render_block::RenderBlockTypeTerrain::m_WasCBApplied). The constant-buffer
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
    /// [`m_BackPatchCullThreshold`](crate::graphics_engine::render_block::RenderBlockTypeTerrainPatch::m_BackPatchCullThreshold), so the hull
    /// shader discards patches whose facing is beyond the threshold relative to that view direction.
    pub m_EnableBackPatchCulling: bool,
    /// Enables frustum patch culling in the color pass. When set (and the active pass is not a shadow
    /// cascade or one of the passes 57..=60), `SetupConstantBuffers` bakes a cull flag into the
    /// hull/domain constant buffer so the hull shader discards patches outside the baked
    /// [`m_OffsetViewProjection`](crate::graphics_engine::graphics_engine::RenderContext::m_OffsetViewProjection)
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
    /// [`m_EnableBackPatchCulling`](crate::graphics_engine::render_block::RenderBlockTypeTerrainPatch::m_EnableBackPatchCulling), baked into
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
    /// [`m_HullProgramHolders`](crate::graphics_engine::render_block::RenderBlockTypeTerrainPatch::m_HullProgramHolders) index 0, other
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
    pub const SetupConstantBuffers_ADDRESS: usize = 0x14032D010;
    /// Binds the type-level constant buffers for a tessellating pass and, once per frame per
    /// constant-buffer slot (the slot comes from the render status and active pass; the stamp is
    /// [`m_WasCBApplied`](crate::graphics_engine::render_block::RenderBlockTypeTerrainPatch::m_WasCBApplied)), bakes the hull/domain constants that the terrain
    /// hull and domain programs read at `cb1`:
    ///
    /// - `float4` 0: `xyz` = the normalized forward vector of the camera manager's *render* camera
    ///   (its `m_TransformF` third basis vector), `w` = `1.0` when the pass carries the color-pass
    ///   render-status bit and [`m_EnableBackPatchCulling`](crate::graphics_engine::render_block::RenderBlockTypeTerrainPatch::m_EnableBackPatchCulling) is set,
    ///   else `0.0`. The hull program discards a patch when all three of its control-point normals
    ///   are beyond [`m_BackPatchCullThreshold`](crate::graphics_engine::render_block::RenderBlockTypeTerrainPatch::m_BackPatchCullThreshold) relative to that one
    ///   direction, so the test approximates the view vector for the whole patch by the camera axis.
    /// - `float4` 1..4: `rc->m_OffsetViewProjection`, the camera-relative view-projection the hull
    ///   program's frustum test projects each control point (expanded by the patch radius along its
    ///   normal) through; a patch whose expanded bounds fall entirely outside one clip plane is
    ///   discarded.
    /// - `float4` 5: `rc->CameraPosition` in `xyz`, with
    ///   [`m_EnableCullByDetail`](crate::graphics_engine::render_block::RenderBlockTypeTerrainPatch::m_EnableCullByDetail) converted to a float in `w`.
    /// - `float4` 6: the camera position again, with the type's tessellation distance in `w`.
    /// - `float4` 7..8: the tessellation factors (inner, edge, `1 / min-spacing`, sphere, normal
    ///   difference), then `m_BackPatchCullThreshold`, then the frustum-cull flag — `1.0` unless the
    ///   active pass is a shadow cascade or one of passes 57..=60, and `0.0` whenever
    ///   [`m_EnableFrustumPatchCulling`](crate::graphics_engine::render_block::RenderBlockTypeTerrainPatch::m_EnableFrustumPatchCulling) is clear — and a
    ///   half-resolution flag fixed at `1.0`.
    ///
    /// The buffer is then bound at hull and domain `cb1`, and the global view-projection constants at
    /// vertex `cb0`.
    pub unsafe fn SetupConstantBuffers(
        &mut self,
        render_context: *mut crate::graphics_engine::graphics_engine::RenderContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                render_context: *mut crate::graphics_engine::graphics_engine::RenderContext,
            ) = ::std::mem::transmute(Self::SetupConstantBuffers_ADDRESS);
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
    /// Issues the impostor cards as a single non-instanced [`DrawIndexed`](crate::graphics_engine::draw::DrawIndexed) over a static quad index
    /// buffer (`6 * min(card_count, 0x1400)` indices); the vertex shader keys off
    /// `SV_VertexID` (`card = id >> 2`, `corner = id & 3`), pulling each card's world position, size, and
    /// atlas data from a texture buffer bound at vertex slot 0. The card orientation (facing) is computed
    /// in the vertex shader from the render camera carried in the engine's global per-view billboard
    /// constant buffer (the translation-bearing `m_ViewProjectionF`); the block stages only per-draw
    /// scalars on `cb1`. `m_RenderStatus & 6` selects the depth/shadow vertex- and fragment-program
    /// permutations. The vertex shader writes `SV_Position` directly; the index and instance counts are
    /// CPU-supplied, so no part of this block's submission is GPU-driven.
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
#[repr(C, align(8))]
/// The water-box render block (`NGraphicsEngine::CWaterBoxRenderBlock`): one bounded water volume.
pub struct WaterBoxRenderBlock {}
impl WaterBoxRenderBlock {
    pub const Draw_ADDRESS: usize = 0x14033B090;
    /// Draws the box volume: the ten-triangle interior hull when the volume-rendering flag is set, and
    /// then — unless the surface-rendering flag is set — the two-triangle top face with the near or
    /// far surface permutation chosen by the camera's distance to the box. Both are plain
    /// [`DrawIndexed`](crate::graphics_engine::draw::DrawIndexed) calls over the type's shared geometry, and both consume the screen-lookup
    /// matrix [`WaterBoxRenderBlockType::Setup`](crate::graphics_engine::render_block::WaterBoxRenderBlockType::Setup) staged for the pass.
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
    pub const DrawSurface_ADDRESS: usize = 0x140355800;
    /// Draws the box's tessellated surface grid (162 triangles) in the surface-rendering mode, staging
    /// the box's own world transform — a scale by the box half-extents plus a camera-relative
    /// translation — on **vertex slot 2, registers 0..3** first. It stages no view-projection; the
    /// screen-lookup matrix is whatever
    /// [`WaterBoxRenderBlockType::Setup`](crate::graphics_engine::render_block::WaterBoxRenderBlockType::Setup) last left on vertex slot 1.
    /// `NWater::DrawWaterBoxSurface` (`0x140_368_C70`) runs the same body inline over every visible
    /// registered water box.
    pub unsafe fn DrawSurface(
        &self,
        rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
            ) = ::std::mem::transmute(Self::DrawSurface_ADDRESS);
            f(self as *const Self as _, rc)
        }
    }
}
impl std::convert::AsRef<WaterBoxRenderBlock> for WaterBoxRenderBlock {
    fn as_ref(&self) -> &WaterBoxRenderBlock {
        self
    }
}
impl std::convert::AsMut<WaterBoxRenderBlock> for WaterBoxRenderBlock {
    fn as_mut(&mut self) -> &mut WaterBoxRenderBlock {
        self
    }
}
#[repr(C, align(8))]
/// The water-box render block *type*
/// (`NGraphicsEngine::CWaterBoxRenderBlock::CWaterBoxRenderBlockType`): the per-pass setup for the
/// `waterbox`, `waterboxbelow`, `waterboxsurface`, and `waterboxclear` permutations that render the
/// bounded water volumes (pools, tanks, interiors) placed in the world as `NWater::SWaterBox`.
pub struct WaterBoxRenderBlockType {}
impl WaterBoxRenderBlockType {
    pub const Setup_ADDRESS: usize = 0x140369020;
    /// Binds the water-box render state, textures, and samplers, stages `cbWaterConsts.WaterConsts` on
    /// **vertex slot 1, registers 4..7**, and hands the fragment stage the inverse view-projection and
    /// the water tuning.
    ///
    /// The vertex constant is
    /// [`RenderContext::m_OffsetViewProjection`](crate::graphics_engine::graphics_engine::RenderContext::m_OffsetViewProjection)
    /// post-multiplied by the same NDC→texture bias
    /// [`WaterHighEndRenderBlockType::Setup`](crate::graphics_engine::render_block::WaterHighEndRenderBlockType::Setup) applies, and it feeds
    /// the same projective-`TEXCOORD1` screen-lookup idiom in the `waterbox*` shaders.
    ///
    /// The whole body is conditional on the water-box manager's mode flags: it runs only when the
    /// volume-rendering flag is set or the surface-rendering flag is clear, so in surface-only mode it
    /// stages nothing and the surface path relies on
    /// [`SetupSurface`](crate::graphics_engine::render_block::WaterBoxRenderBlockType::SetupSurface) instead.
    pub unsafe fn Setup(
        &self,
        rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
        vertex_declaration: *mut crate::graphics_engine::graphics_engine::HVertexDeclaration_t,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
                vertex_declaration: *mut crate::graphics_engine::graphics_engine::HVertexDeclaration_t,
            ) = ::std::mem::transmute(Self::Setup_ADDRESS);
            f(self as *const Self as _, rc, vertex_declaration)
        }
    }
    pub const SetupSurface_ADDRESS: usize = 0x14033B210;
    /// The alternative setup for the surface-rendering mode: binds the tessellated surface grid's
    /// vertex declaration, stream, and index buffer, sets the stencil test that keeps the surface
    /// inside the box, and sizes the vertex constant buffer at slot 2. Stages no view-projection of
    /// its own.
    pub unsafe fn SetupSurface(
        &self,
        rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
        vertex_declaration: *mut crate::graphics_engine::graphics_engine::HVertexDeclaration_t,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
                vertex_declaration: *mut crate::graphics_engine::graphics_engine::HVertexDeclaration_t,
            ) = ::std::mem::transmute(Self::SetupSurface_ADDRESS);
            f(self as *const Self as _, rc, vertex_declaration)
        }
    }
}
impl std::convert::AsRef<WaterBoxRenderBlockType> for WaterBoxRenderBlockType {
    fn as_ref(&self) -> &WaterBoxRenderBlockType {
        self
    }
}
impl std::convert::AsMut<WaterBoxRenderBlockType> for WaterBoxRenderBlockType {
    fn as_mut(&mut self) -> &mut WaterBoxRenderBlockType {
        self
    }
}
#[repr(C, align(8))]
/// The legacy high-end water render block (`NGraphicsEngine::CWaterHighEndRenderBlock`): one water
/// patch of the ocean/lake surface grid, drawn with the shader permutations set up by
/// [`WaterHighEndRenderBlockType`](crate::graphics_engine::render_block::WaterHighEndRenderBlockType).
pub struct WaterHighEndRenderBlock {}
impl WaterHighEndRenderBlock {
    pub const Draw_ADDRESS: usize = 0x140356CC0;
    /// Selects the vertex/fragment program permutation from the patch's distance to the camera (a far
    /// pair beyond 1024 m, otherwise one of three LOD pairs), stages the patch's own position and
    /// scale on vertex slot 2 register 0, binds the shared water vertex stream, and issues the patch's
    /// quads through the block's internal `DrawQuads` ([`DrawIndexed`](crate::graphics_engine::draw::DrawIndexed) per admitted quad). The
    /// screen-space lookup matrix it draws with is the one
    /// [`WaterHighEndRenderBlockType::Setup`](crate::graphics_engine::render_block::WaterHighEndRenderBlockType::Setup) staged for the pass;
    /// nothing here restages it.
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
}
impl std::convert::AsRef<WaterHighEndRenderBlock> for WaterHighEndRenderBlock {
    fn as_ref(&self) -> &WaterHighEndRenderBlock {
        self
    }
}
impl std::convert::AsMut<WaterHighEndRenderBlock> for WaterHighEndRenderBlock {
    fn as_mut(&mut self) -> &mut WaterHighEndRenderBlock {
        self
    }
}
#[repr(C, align(8))]
/// The legacy (non-NVIDIA WaveWorks) high-end water render block *type*
/// (`NGraphicsEngine::CWaterHighEndRenderBlock::CWaterHighEndRenderBlockType`): the shared per-pass
/// setup for the `waterhighend`, `waterbelow`, and `watershader_lod0/1/2` shader permutations, which
/// the engine selects at the lower water-quality settings in place of the WaveWorks path.
pub struct WaterHighEndRenderBlockType {}
impl WaterHighEndRenderBlockType {
    pub const Setup_ADDRESS: usize = 0x1403692E0;
    /// Binds the water render state (alpha blend, depth test and write, back-face cull), the five
    /// water textures and samplers, the wave-table vertex constants, and the fragment constants for
    /// the water tuning — then stages the block type's `TypeConstants.ReflectionViewProj` on **vertex
    /// slot 1, registers 1..4**.
    ///
    /// That matrix is
    /// [`RenderContext::m_ViewProjectionF`](crate::graphics_engine::graphics_engine::RenderContext::m_ViewProjectionF)
    /// post-multiplied (row-vector convention, so the bias applies to the projected result) by the
    /// constant NDC→texture bias
    /// `{0.5,0,0,0 / 0,0.5,0,0 / 0,0,1,0 / 0.5,0.5,0,1}`. The vertex shaders transform the water
    /// vertex by it with a multiply-add chain over those four registers and pass the `(u·w, v·w, w)`
    /// components on as a projective `TEXCOORD1`; the pixel shaders divide by `w` to get the
    /// screen-space UV they sample `ReflectionMap`, `RefractionMap`, and `DepthMap` with. The NDC→UV
    /// half-scale is therefore already folded into the CPU-side matrix rather than done in the shader,
    /// and the UV it produces is normalized over the *viewport* the matrix was built for.
    ///
    /// The fragment stage separately receives the inverse of
    /// [`RenderContext::m_ViewProjectionF`](crate::graphics_engine::graphics_engine::RenderContext::m_ViewProjectionF)
    /// at fragment slot 1 registers 3..6 for its own depth reconstruction.
    pub unsafe fn Setup(
        &self,
        rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
        vertex_declaration: *mut crate::graphics_engine::graphics_engine::HVertexDeclaration_t,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                rc: *mut crate::graphics_engine::graphics_engine::RenderContext,
                vertex_declaration: *mut crate::graphics_engine::graphics_engine::HVertexDeclaration_t,
            ) = ::std::mem::transmute(Self::Setup_ADDRESS);
            f(self as *const Self as _, rc, vertex_declaration)
        }
    }
}
impl std::convert::AsRef<WaterHighEndRenderBlockType> for WaterHighEndRenderBlockType {
    fn as_ref(&self) -> &WaterHighEndRenderBlockType {
        self
    }
}
impl std::convert::AsMut<WaterHighEndRenderBlockType> for WaterHighEndRenderBlockType {
    fn as_mut(&mut self) -> &mut WaterHighEndRenderBlockType {
        self
    }
}
pub const WaveWorksSimulationStep_ADDRESS: usize = 0x140336CE0;
/// Advances the WaveWorks ocean simulation one step and blocks until its displacement readback is
/// available (`GFSDK_WaveWorks_Simulation_Simulation` — a helper in the game's own image, named by
/// its telemetry zone, not an export of `gfsdk_waveworks.win64.dll`).
///
/// Sets the simulation time, then loops `WaitStagingCursor` + `KickD3D11` until the staging cursor
/// reports the readback is no longer in flight, and archives the resulting displacement snapshot —
/// which is what the CPU-side wave-height and buoyancy queries
/// (`GFSDK_WaveWorks_Simulation_GetArchivedDisplacements`) read. Finally restores the D3D state the
/// kick changed from the shared savestate.
///
/// Called once per frame from [`NvWaterHighEndRenderBlock::Draw`](crate::graphics_engine::render_block::NvWaterHighEndRenderBlock::Draw).
/// It is not idempotent: each call archives another displacement snapshot and can block on the
/// staging cursor.
pub unsafe fn WaveWorksSimulationStep(
    render_time: f64,
    gfx_context: *mut ::std::ffi::c_void,
    kick_id: *mut u64,
    simulation: *mut ::std::ffi::c_void,
    savestate: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            render_time: f64,
            gfx_context: *mut ::std::ffi::c_void,
            kick_id: *mut u64,
            simulation: *mut ::std::ffi::c_void,
            savestate: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(WaveWorksSimulationStep_ADDRESS);
        f(render_time, gfx_context, kick_id, simulation, savestate)
    }
}
