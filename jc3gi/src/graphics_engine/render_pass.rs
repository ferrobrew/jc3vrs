#![allow(
    dead_code,
    non_snake_case,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast
)]
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
/// One of a render pass's double-buffered render-block-item lists. Add appends entries
/// (InterlockedExchangeAdd on m_NumElements); on overflow (m_NumElements >= m_ListSize) it spills
/// into the global overflow list. DoDraw reads min(m_ListSize, m_NumElements) entries and never
/// writes m_NumElements, so a populated list can be redrawn any number of times.
pub struct RBILists {
    /// Array of 0x20-byte entries.
    pub m_List: *mut ::std::ffi::c_void,
    /// Capacity.
    pub m_ListSize: u16,
    _field_a: [u8; 2],
    /// Live element count (volatile).
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
    /// Appends one render-block-item; spills to the global overflow list on capacity overflow.
    pub unsafe fn Add(
        &mut self,
        a2: i32,
        block: *mut ::std::ffi::c_void,
        info: *mut ::std::ffi::c_void,
    ) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                a2: i32,
                block: *mut ::std::ffi::c_void,
                info: *mut ::std::ffi::c_void,
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
pub struct RenderPass {}
impl RenderPass {
    pub const SetupRenderFrameData_ADDRESS: usize = 0x14048C4E0;
    /// Appends `count` render-block-items (`items`) to the active RBILists draw/add lists (`a3`
    /// holds the lists). Static (no `this`). Called per batch, including from CPU fragment worker
    /// threads -- not once per frame. The argument list matters: it dereferences `a3 + 0x8038`.
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
    /// Fills the per-view RenderContext from `camera` (the render camera, or a shadow/reflective
    /// light camera selected by the pass flags): view, projection and offset (translation-free)
    /// view-projection via CalculateOffsetViewProjectionMatrix, the separate camera world position,
    /// and the parity-buffered shadow matrices ((render_frame_counters.m_FrameIndex & 1) stride).
    /// Static (no `this`).
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
    /// The per-pass half of the list rotation (vtable slot 3, driven by RotateRenderFrameData):
    /// points m_CurrentAddList at m_Lists[parity], m_CurrentDrawList at the other, and zeroes the
    /// new add-list's element count. Also snapshots the pass camera.
    pub unsafe fn SaveRenderFrameData(&mut self, parity: u32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, parity: u32) = ::std::mem::transmute(
                Self::SaveRenderFrameData_ADDRESS,
            );
            f(self as *mut Self as _, parity)
        }
    }
    pub const DoDraw_ADDRESS: usize = 0x1401AC7A0;
    /// Draws m_CurrentDrawList: walks min(m_ListSize, m_NumElements) blocks via a local cursor and
    /// vtable-dispatches each. Non-destructive -- never writes m_NumElements (vtable slot 2).
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
pub unsafe fn get_render_block_overflow_count() -> &'static mut u32 {
    unsafe { &mut *(0x142ED0FA0 as *mut u32) }
}
pub const CalculateOffsetViewProjectionMatrix_ADDRESS: usize = 0x140136020;
/// Copies `src` (a view matrix), zeroes its translation row ({0,0,0,1}), and multiplies by `proj`,
/// writing the translation-free OffsetViewProjection into `dst`. This is the view-projection opaque
/// (camera-relative) geometry actually uses; the camera world position is supplied separately.
/// Static (no `this`).
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
pub const RotateRenderFrameData_ADDRESS: usize = 0x1401A3000;
/// The per-frame render-block-item list rotation, run once in each `CGraphicsEngine::Draw`
/// (0x1400F4170) prologue (its call site at 0x1400F4340 is mislabeled `CKeep1000Frames` in this
/// binary's symbols). Toggles the global add/draw parity at `0x142ED7680`, then for every render
/// pass swaps `m_CurrentAddList`/`m_CurrentDrawList` to the new parity (via `RenderPass`'s vtable
/// slot 3) and zeroes the new add-list's element count, and finally flushes the render-block-item
/// overflow list. Static (no `this`); reads the render engine + parity from globals. This -- not the
/// per-batch `SetupRenderFrameData` build above -- is the actual draw-list swap.
pub unsafe fn RotateRenderFrameData() {
    unsafe {
        let f: unsafe extern "system" fn() = ::std::mem::transmute(
            RotateRenderFrameData_ADDRESS,
        );
        f()
    }
}
