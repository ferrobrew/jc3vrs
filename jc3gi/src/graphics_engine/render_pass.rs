#![cfg_attr(any(), rustfmt::skip)]
#[allow(unused_imports)]
use crate::graphics_engine::shadow_manager::ShadowManager;
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
    pub m_List: *mut ::std::ffi::c_void,
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
/// One render block enqueued onto a draw list. Opaque; handled only behind pointers.
pub struct RenderBlock {}
impl RenderBlock {}
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
pub struct RenderPass {
    _field_0: [u8; 40],
    /// The list that new render-block-items append to this frame. Each rotation,
    /// [`SaveRenderFrameData`](RenderPass::SaveRenderFrameData) re-points it at the new parity's list
    /// and zeroes its count; zeroing that count is how a pass's draw-time additions are reset.
    pub m_CurrentAddList: *mut crate::graphics_engine::render_pass::RBILists,
    _field_30: [u8; 94],
    /// Pass state flags. [`m_Enabled`](RenderPassState::m_Enabled) gates whether the pass draws;
    /// [`ShadowManager::CommitRenderPassSettings`](graphics_engine::shadow_manager::ShadowManager::CommitRenderPassSettings)
    /// drives it per dispatch for the shadow passes.
    pub m_StateFlags: crate::graphics_engine::render_pass::RenderPassState,
}
fn _RenderPass_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x90], RenderPass>([0u8; 0x90]);
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
    /// vtable-dispatching each. Non-destructive -- never writes `m_NumElements`.
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
