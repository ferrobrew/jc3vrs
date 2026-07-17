#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The per-frame constant-buffer ring.
pub struct ConstantBufferPool {}
impl ConstantBufferPool {
    pub const HandBackBuffers_ADDRESS: usize = 0x1400E04F0;
    /// Recycles last frame's constant buffers from the in-use stack back to the free pool.
    pub unsafe fn HandBackBuffers(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::HandBackBuffers_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<ConstantBufferPool> for ConstantBufferPool {
    fn as_ref(&self) -> &ConstantBufferPool {
        self
    }
}
impl std::convert::AsMut<ConstantBufferPool> for ConstantBufferPool {
    fn as_mut(&mut self) -> &mut ConstantBufferPool {
        self
    }
}
#[repr(C, align(8))]
/// The low-resolution particle render pass: draws the particle render blocks routed to it (see
/// `m_LowResRendering` on the particle block type) into a reduced-resolution target that the low-res
/// upsampling pass later composites back up.
pub struct LRParticleRenderPass {}
impl LRParticleRenderPass {
    pub const Draw_ADDRESS: usize = 0x1400A4170;
    /// Binds the reduced-resolution render setup, runs the base [`RenderPass::DoDraw`] over the routed
    /// particle blocks, and restores the previously bound render setup.
    pub unsafe fn Draw(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Draw_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<LRParticleRenderPass> for LRParticleRenderPass {
    fn as_ref(&self) -> &LRParticleRenderPass {
        self
    }
}
impl std::convert::AsMut<LRParticleRenderPass> for LRParticleRenderPass {
    fn as_mut(&mut self) -> &mut LRParticleRenderPass {
        self
    }
}
#[repr(C, align(8))]
/// The per-item info accompanying a render block on a draw list (transform, sort key, and the like).
/// Opaque; handled only behind pointers.
pub struct RBIInfo {}
impl RBIInfo {}
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
/// One of a render pass's double-buffered render-block-item lists.
///
/// [`Add`](RBILists::Add) appends entries; on overflow (`m_NumElements >= m_ListSize`) it spills into
/// the global overflow list. [`DoDraw`](RenderPass::DoDraw) reads `min(m_ListSize, m_NumElements)`
/// entries and never writes `m_NumElements`, so a populated list can be redrawn any number of times.
pub struct RBILists {
    /// The array of entries.
    pub m_List: *mut crate::graphics_engine::render_pass::RenderInstance,
    /// The capacity.
    pub m_ListSize: u16,
    _field_a: [u8; 2],
    /// The live element count (volatile).
    pub m_NumElements: u32,
}
fn _RBILists_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], RBILists>([0u8; 0x10]);
    }
    unreachable!()
}
impl RBILists {
    pub const Add_ADDRESS: usize = 0x14011C070;
    /// Appends one render-block-item, spilling to the global overflow list on capacity overflow.
    pub unsafe fn Add(
        &mut self,
        a2: i32,
        block: *mut crate::graphics_engine::render_pass::RenderBlock,
        info: *mut crate::graphics_engine::render_pass::RBIInfo,
    ) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                a2: i32,
                block: *mut crate::graphics_engine::render_pass::RenderBlock,
                info: *mut crate::graphics_engine::render_pass::RBIInfo,
            ) -> u64 = ::std::mem::transmute(Self::Add_ADDRESS);
            f(self as *mut Self as _, a2, block, info)
        }
    }
}
impl std::convert::AsRef<RBILists> for RBILists {
    fn as_ref(&self) -> &RBILists {
        self
    }
}
impl std::convert::AsMut<RBILists> for RBILists {
    fn as_mut(&mut self) -> &mut RBILists {
        self
    }
}
#[repr(C, align(8))]
/// One render block enqueued onto a draw list (`NGraphicsEngine::IRenderBlock`). Mapped by vtable
/// only; instances are handled behind pointers.
pub struct RenderBlock {
    vftable: *const crate::graphics_engine::render_pass::RenderBlockVftable,
}
fn _RenderBlock_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], RenderBlock>([0u8; 0x8]);
    }
    unreachable!()
}
impl RenderBlock {
    pub fn vftable(
        &self,
    ) -> *const crate::graphics_engine::render_pass::RenderBlockVftable {
        self.vftable as *const crate::graphics_engine::render_pass::RenderBlockVftable
    }
    /// Returns the block's [`RenderBlockTypeBase`]. [`RenderPass::DoDraw`] calls this per
    /// entry to detect type runs and drive the type-level setup/restore.
    pub unsafe fn GetType(
        &mut self,
    ) -> *mut crate::graphics_engine::render_engine::RenderBlockTypeBase {
        unsafe {
            let f = (&raw const (*self.vftable()).GetType).read();
            f(self as *mut Self as _)
        }
    }
    /// Returns the block's 64-bit sort identifier (typically derived from its shaders and
    /// material so equal-state blocks batch together). The sort-ID computation passes store
    /// the result in [`RenderInstance::m_SortID`].
    pub unsafe fn GetSortID(
        &mut self,
        sc: *const crate::graphics_engine::render_pass::SortContext,
        info: *const crate::graphics_engine::render_pass::RBIInfo,
    ) -> u64 {
        unsafe {
            let f = (&raw const (*self.vftable()).GetSortID).read();
            f(self as *mut Self as _, sc, info)
        }
    }
    /// Returns the squared distance from the instance's world translation
    /// (`info.m_Transform[sc.m_RenderFrameIndex]`) to [`SortContext::m_CameraPosition`]. The
    /// depth-aware sort-ID computations store the result (or its depth-bucket index) in
    /// [`RenderInstance::m_Depth`].
    pub unsafe fn GetSqDistanceToCamera(
        &mut self,
        sc: *const crate::graphics_engine::render_pass::SortContext,
        info: *const crate::graphics_engine::render_pass::RBIInfo,
    ) -> f32 {
        unsafe {
            let f = (&raw const (*self.vftable()).GetSqDistanceToCamera).read();
            f(self as *mut Self as _, sc, info)
        }
    }
    /// Per-block state setup for the colour path, called by [`RenderPass::DoDraw`] when the
    /// block pointer changes within a type run.
    pub unsafe fn Setup(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
        sort_id: u64,
        a4: u64,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).Setup).read();
            f(self as *mut Self as _, ctx, sort_id, a4)
        }
    }
    /// Per-block state setup for the depth-only path (the Z passes).
    pub unsafe fn SetupZ(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
        sort_id: u64,
        a4: u64,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).SetupZ).read();
            f(self as *mut Self as _, ctx, sort_id, a4)
        }
    }
    /// Issues the block's draw calls for the colour path.
    pub unsafe fn Draw(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
        info: *const crate::graphics_engine::render_pass::RBIInfo,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).Draw).read();
            f(self as *mut Self as _, ctx, info)
        }
    }
    /// Issues the block's draw calls for the depth-only path (the Z passes).
    pub unsafe fn DrawZ(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
        info: *const crate::graphics_engine::render_pass::RBIInfo,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).DrawZ).read();
            f(self as *mut Self as _, ctx, info)
        }
    }
    /// Writes the block's local-space bounding box; returns whether one is available.
    pub unsafe fn GetBoundingBox(
        &mut self,
        min: *mut crate::types::math::Vector3,
        max: *mut crate::types::math::Vector3,
    ) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).GetBoundingBox).read();
            f(self as *mut Self as _, min, max)
        }
    }
}
impl std::convert::AsRef<RenderBlock> for RenderBlock {
    fn as_ref(&self) -> &RenderBlock {
        self
    }
}
impl std::convert::AsMut<RenderBlock> for RenderBlock {
    fn as_mut(&mut self) -> &mut RenderBlock {
        self
    }
}
#[repr(C, align(8))]
pub struct RenderBlockVftable {
    _vfunc_0: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_1: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_2: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_3: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_4: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_5: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_6: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_7: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_8: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_9: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_10: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_11: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    /// Returns the block's [`RenderBlockTypeBase`]. [`RenderPass::DoDraw`] calls this per
    /// entry to detect type runs and drive the type-level setup/restore.
    pub GetType: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ) -> *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
    _vfunc_13: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_14: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_15: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_16: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_17: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_18: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_19: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_20: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_21: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_22: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_23: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_24: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    _vfunc_25: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    /// Returns the block's 64-bit sort identifier (typically derived from its shaders and
    /// material so equal-state blocks batch together). The sort-ID computation passes store
    /// the result in [`RenderInstance::m_SortID`].
    pub GetSortID: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
        sc: *const crate::graphics_engine::render_pass::SortContext,
        info: *const crate::graphics_engine::render_pass::RBIInfo,
    ) -> u64,
    /// Returns the squared distance from the instance's world translation
    /// (`info.m_Transform[sc.m_RenderFrameIndex]`) to [`SortContext::m_CameraPosition`]. The
    /// depth-aware sort-ID computations store the result (or its depth-bucket index) in
    /// [`RenderInstance::m_Depth`].
    pub GetSqDistanceToCamera: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
        sc: *const crate::graphics_engine::render_pass::SortContext,
        info: *const crate::graphics_engine::render_pass::RBIInfo,
    ) -> f32,
    /// Per-block state setup for the colour path, called by [`RenderPass::DoDraw`] when the
    /// block pointer changes within a type run.
    pub Setup: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
        ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
        sort_id: u64,
        a4: u64,
    ),
    /// Per-block state setup for the depth-only path (the Z passes).
    pub SetupZ: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
        ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
        sort_id: u64,
        a4: u64,
    ),
    /// Issues the block's draw calls for the colour path.
    pub Draw: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
        ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
        info: *const crate::graphics_engine::render_pass::RBIInfo,
    ),
    /// Issues the block's draw calls for the depth-only path (the Z passes).
    pub DrawZ: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
        ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
        info: *const crate::graphics_engine::render_pass::RBIInfo,
    ),
    _vfunc_32: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
    ),
    /// Writes the block's local-space bounding box; returns whether one is available.
    pub GetBoundingBox: unsafe extern "system" fn(
        this: *mut crate::graphics_engine::render_pass::RenderBlock,
        min: *mut crate::types::math::Vector3,
        max: *mut crate::types::math::Vector3,
    ) -> bool,
}
fn _RenderBlockVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x110], RenderBlockVftable>([0u8; 0x110]);
    }
    unreachable!()
}
impl RenderBlockVftable {}
impl std::convert::AsRef<RenderBlockVftable> for RenderBlockVftable {
    fn as_ref(&self) -> &RenderBlockVftable {
        self
    }
}
impl std::convert::AsMut<RenderBlockVftable> for RenderBlockVftable {
    fn as_mut(&mut self) -> &mut RenderBlockVftable {
        self
    }
}
#[derive(Copy, Clone)]
#[repr(C, align(8))]
/// One draw-list entry (`NGraphicsEngine::SRenderInstance`): a render block plus its per-instance
/// info and the sort keys the per-frame sort orders it by.
pub struct RenderInstance {
    /// The block's sort identifier, written per frame by the sort-ID computation in
    /// [`RenderPass::SortList`] from [`GetSortID`](RenderBlock::GetSortID). The final ordering key
    /// after depth and type.
    pub m_SortID: u64,
    /// The render block.
    pub m_RenderBlock: *mut crate::graphics_engine::render_pass::RenderBlock,
    /// The per-instance info handed to the block's draw call.
    pub m_Info: *mut crate::graphics_engine::render_pass::RBIInfo,
    /// The block's type index, the second ordering key. Written by [`RBILists::Add`].
    pub m_RenderBlockType: i32,
    /// The primary ordering key, written per frame by the sort-ID computation in
    /// [`RenderPass::SortList`]: zero for non-depth sorts, the raw squared camera distance for
    /// `FrontToBack`/`BackToFront`, or the depth-bucket index (see
    /// [`RenderPass::m_DepthSqTable`]) for `FrontToBackBucketed`.
    pub m_Depth: f32,
}
fn _RenderInstance_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], RenderInstance>([0u8; 0x20]);
    }
    unreachable!()
}
impl RenderInstance {}
impl std::convert::AsRef<RenderInstance> for RenderInstance {
    fn as_ref(&self) -> &RenderInstance {
        self
    }
}
impl std::convert::AsMut<RenderInstance> for RenderInstance {
    fn as_mut(&mut self) -> &mut RenderInstance {
        self
    }
}
#[repr(C, align(8))]
/// The base render pass (`NGraphicsEngine::CRenderPass`, 0x8C0 bytes in the release build).
/// Specialized passes derive from it, so only the base extent is claimed here.
pub struct RenderPass {
    _field_0: [u8; 40],
    /// The list that new render-block-items append to this frame. Each rotation,
    /// [`SaveRenderFrameData`](RenderPass::SaveRenderFrameData) re-points it at the new parity's list
    /// and zeroes its count; zeroing that count is how a pass's draw-time additions are reset.
    pub m_CurrentAddList: *mut crate::graphics_engine::render_pass::RBILists,
    /// The list [`DoDraw`](RenderPass::DoDraw) draws this frame: the other parity's list, populated
    /// by last rotation's adds. Re-pointed alongside
    /// [`m_CurrentAddList`](RenderPass::m_CurrentAddList) by
    /// [`SaveRenderFrameData`](RenderPass::SaveRenderFrameData).
    pub m_CurrentDrawList: *mut crate::graphics_engine::render_pass::RBILists,
    _field_38: [u8; 16],
    /// Whether the pass renders back-to-front (the alpha-blend render setting). The `Auto` sort
    /// method resolves to `BackToFront` when set, `FrontToBackBucketed` otherwise.
    pub m_RenderBackToFront: i32,
    _field_4c: [u8; 66],
    /// Pass state flags. [`m_Enabled`](RenderPassState::m_Enabled) gates whether the pass draws;
    /// [`ShadowManager::CommitRenderPassSettings`](graphics_engine::shadow_manager::ShadowManager::CommitRenderPassSettings)
    /// drives it per dispatch for the shadow passes.
    pub m_StateFlags: crate::graphics_engine::render_pass::RenderPassState,
    _field_90: [u8; 12],
    /// List/sort state flags; carries the [`m_Sorted`](RenderPassSortState::m_Sorted) latch.
    pub m_SortStateFlags: crate::graphics_engine::render_pass::RenderPassSortState,
    /// The pass's [`RenderPassId`], stored narrow.
    pub m_Index: i16,
    _field_a0: [u8; 1976],
    /// The spinlock serializing [`SortList`](RenderPass::SortList) against concurrent callers
    /// (the render-engine sort task and the lazy sort in [`DoDraw`](RenderPass::DoDraw)).
    pub m_SortLock: *mut ::std::ffi::c_void,
    _field_860: [u8; 24],
    /// How [`SortList`](RenderPass::SortList) orders the draw list. Most passes leave the
    /// constructor default (`Auto`); passes that opt out set `None`.
    pub m_SortMethod: crate::graphics_engine::render_pass::RenderPassSortMethod,
    /// The number of live entries in [`m_DepthSqTable`](RenderPass::m_DepthSqTable). The
    /// constructor initializes a single zero bucket; with one bucket the bucketed sort computes no
    /// depth keys.
    pub m_NumDepthBuckets: u16,
    _field_87e: [u8; 2],
    /// The depth-bucket boundary table for `FrontToBackBucketed` sorting: ascending *squared*
    /// minimum distances, one per bucket, up to 16. An instance's bucket is the first entry its
    /// squared camera distance is below. Boundaries are registered at pass creation by squaring
    /// the boundary distance, appending it, and re-sorting the live prefix (the engine's
    /// `AddDepthBucket`, inlined into `CRenderEngine::InitializeSystem` in the release build); the
    /// only stock user is the Z-and-velocity pass, with boundaries at 40 and 120 metres.
    pub m_DepthSqTable: [f32; 16],
}
fn _RenderPass_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8C0], RenderPass>([0u8; 0x8C0]);
    }
    unreachable!()
}
impl RenderPass {
    pub const SetupRenderFrameData_ADDRESS: usize = 0x14048C4E0;
    /// Appends `count` render-block-items (`items`) to the active draw / add lists (`a3` holds the
    /// lists). Static. Called per batch, including from CPU fragment worker threads, not once per
    /// frame.
    pub unsafe fn SetupRenderFrameData(
        a1: *mut ::std::ffi::c_void,
        count: i32,
        a3: *mut ::std::ffi::c_void,
        items: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                a1: *mut ::std::ffi::c_void,
                count: i32,
                a3: *mut ::std::ffi::c_void,
                items: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::SetupRenderFrameData_ADDRESS);
            f(a1, count, a3, items)
        }
    }
    pub const SetRenderContextCamera_ADDRESS: usize = 0x140187430;
    /// Fills the per-view render context from `camera` (the render camera, or a shadow or reflective
    /// light camera selected by the pass flags): the view, projection, and translation-free offset
    /// view-projection via [`CalculateOffsetViewProjectionMatrix`], the separate camera world
    /// position, and the parity-buffered shadow matrices. Static.
    pub unsafe fn SetRenderContextCamera(
        ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
        camera: *const crate::camera::camera::Camera,
    ) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(
                ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
                camera: *const crate::camera::camera::Camera,
            ) -> u64 = ::std::mem::transmute(Self::SetRenderContextCamera_ADDRESS);
            f(ctx, camera)
        }
    }
    pub const SaveRenderFrameData_ADDRESS: usize = 0x140194480;
    /// The per-pass half of the list rotation, driven by the per-frame `CKeep1000Frames` call in the
    /// `CGraphicsEngine::Draw` prologue: points `m_CurrentAddList` at the new parity's list, the draw
    /// list at the other, and zeroes the new add-list's element count. Also snapshots the pass camera.
    pub unsafe fn SaveRenderFrameData(&mut self, parity: u32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, parity: u32) = ::std::mem::transmute(
                Self::SaveRenderFrameData_ADDRESS,
            );
            f(self as *mut Self as _, parity)
        }
    }
    pub const DoDraw_ADDRESS: usize = 0x1401AC7A0;
    /// Draws the current draw list, walking `min(m_ListSize, m_NumElements)` blocks and
    /// vtable-dispatching each. Non-destructive -- never writes `m_NumElements`. Calls
    /// [`SortList`](RenderPass::SortList) lazily before drawing. Consecutive blocks of one type
    /// form a type run: [`ChangeRenderBlockType`](RenderPass::ChangeRenderBlockType) switches
    /// between runs, and the tail closes the final run (its type-level restore and its scope
    /// marker).
    pub unsafe fn DoDraw(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
        color_mask: u32,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
                color_mask: u32,
            ) -> bool = ::std::mem::transmute(Self::DoDraw_ADDRESS);
            f(self as *mut Self as _, ctx, color_mask)
        }
    }
    pub const ChangeRenderBlockType_ADDRESS: usize = 0x140187310;
    /// Switches the active render-block type between type runs in [`DoDraw`](RenderPass::DoDraw):
    /// restores `prev` (dispatching the Z, colour, or glint restore variant selected by the pass
    /// id) and closes its scope marker (`Graphics::EndScopeMarker`), then re-applies the
    /// pass render states, sets up `next`, and opens a scope marker (`Graphics::BeginScopeMarker`)
    /// named by its
    /// [`GetTypeName`](graphics_engine::render_engine::RenderBlockTypeBase::GetTypeName). Either
    /// pointer may be null at a run boundary. `inout_count` is the pass's running drawn-block
    /// counter, zeroed on a type switch.
    pub unsafe fn ChangeRenderBlockType(
        &mut self,
        ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
        prev: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
        next: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
        inout_count: *mut u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::RenderContext,
                prev: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
                next: *mut crate::graphics_engine::render_engine::RenderBlockTypeBase,
                inout_count: *mut u32,
            ) = ::std::mem::transmute(Self::ChangeRenderBlockType_ADDRESS);
            f(self as *mut Self as _, ctx, prev, next, inout_count)
        }
    }
    pub const SortList_ADDRESS: usize = 0x1401A87C0;
    /// Sorts the current draw list by ([`m_Depth`](RenderInstance::m_Depth),
    /// [`m_RenderBlockType`](RenderInstance::m_RenderBlockType),
    /// [`m_SortID`](RenderInstance::m_SortID)) according to
    /// [`m_SortMethod`](RenderPass::m_SortMethod), first recomputing each entry's depth and
    /// sort-ID keys through the block vtable
    /// ([`GetSortID`](RenderBlock::GetSortID) / [`GetSqDistanceToCamera`](RenderBlock::GetSqDistanceToCamera)).
    /// Latches [`m_Sorted`](RenderPassSortState::m_Sorted) under [`m_SortLock`](RenderPass::m_SortLock),
    /// so the sort runs at most once per list rotation; both the render-engine sort task
    /// (`CRenderEngine::SortRenderPasses`) and the lazy call in [`DoDraw`](RenderPass::DoDraw)
    /// funnel here.
    pub unsafe fn SortList(
        &mut self,
        sc: *const crate::graphics_engine::render_pass::SortContext,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                sc: *const crate::graphics_engine::render_pass::SortContext,
            ) = ::std::mem::transmute(Self::SortList_ADDRESS);
            f(self as *mut Self as _, sc)
        }
    }
}
impl std::convert::AsRef<RenderPass> for RenderPass {
    fn as_ref(&self) -> &RenderPass {
        self
    }
}
impl std::convert::AsMut<RenderPass> for RenderPass {
    fn as_mut(&mut self) -> &mut RenderPass {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// How a render pass orders its draw list (`NGraphicsEngine::ERenderPassSortMethod`).
pub enum RenderPassSortMethod {
    RenderPassSortMethod_Auto = 0isize as _,
    RenderPassSortMethod_None = 1isize as _,
    RenderPassSortMethod_SortID = 2isize as _,
    RenderPassSortMethod_BackToFront = 3isize as _,
    RenderPassSortMethod_FrontToBack = 4isize as _,
    RenderPassSortMethod_FrontToBackBucketed = 5isize as _,
}
fn _RenderPassSortMethod_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], RenderPassSortMethod>([0u8; 0x4]);
    }
    unreachable!()
}
crate::__bitflags! {
    #[doc =
    " Pass list/sort state flags. Only the sorted latch is identified; the remaining bits are"]
    #[doc = " unmapped."] pub struct RenderPassSortState : u16 { const m_Sorted =
    1024usize as _; }
}
fn _RenderPassSortState_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x2], RenderPassSortState>([0u8; 0x2]);
    }
    unreachable!()
}
crate::__bitflags! {
    #[doc =
    " Render-pass state flags. Only the draw gate is identified; the remaining bits are unmapped."]
    pub struct RenderPassState : u16 { const m_Enabled = 8usize as _; }
}
fn _RenderPassState_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x2], RenderPassState>([0u8; 0x2]);
    }
    unreachable!()
}
#[derive(Copy, Clone)]
#[repr(C, align(8))]
/// The camera context a render pass sorts against (`NGraphicsEngine::IRenderBlock::SSortContext`).
/// [`RenderPass::SortList`] receives it from either of its two callers:
/// `CRenderEngine::SortRenderPasses` builds it from the pass's sort camera (its external camera if
/// set, else the render camera), and [`RenderPass::DoDraw`]'s lazy call builds it from the render
/// context's camera fields.
pub struct SortContext {
    /// The [`RenderPassId`] of the pass being sorted.
    pub m_RenderPassIndex: i32,
    /// The `%3` render-frame ring index selecting which of the info's buffered world transforms
    /// the distance query reads.
    pub m_RenderFrameIndex: u32,
    /// The sort camera's world position.
    pub m_CameraPosition: crate::types::math::Vector3,
    /// The sort camera's near-plane distance.
    pub m_CameraNear: f32,
}
fn _SortContext_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], SortContext>([0u8; 0x18]);
    }
    unreachable!()
}
impl SortContext {}
impl std::convert::AsRef<SortContext> for SortContext {
    fn as_ref(&self) -> &SortContext {
        self
    }
}
impl std::convert::AsMut<SortContext> for SortContext {
    fn as_mut(&mut self) -> &mut SortContext {
        self
    }
}
pub unsafe fn get_current_add_buffer() -> &'static mut u32 {
    unsafe { &mut *(0x142ED7680 as *mut u32) }
}
pub unsafe fn get_render_block_overflow_count() -> &'static mut u32 {
    unsafe { &mut *(0x142ED0FA0 as *mut u32) }
}
pub const CalculateOffsetViewProjectionMatrix_ADDRESS: usize = 0x140136020;
/// Copies `src` (a view matrix), zeroes its translation row, and multiplies by `proj`, writing the
/// translation-free offset view-projection into `dst`. This is the view-projection that camera-relative
/// opaque geometry actually uses; the camera world position is supplied separately. Static.
pub unsafe fn CalculateOffsetViewProjectionMatrix(
    src: *const crate::types::math::Matrix4,
    proj: *const crate::types::math::Matrix4,
    dst: *mut crate::types::math::Matrix4,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            src: *const crate::types::math::Matrix4,
            proj: *const crate::types::math::Matrix4,
            dst: *mut crate::types::math::Matrix4,
        ) = ::std::mem::transmute(CalculateOffsetViewProjectionMatrix_ADDRESS);
        f(src, proj, dst)
    }
}
