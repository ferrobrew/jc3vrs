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
        first: u32,
        last: u32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                ctx: *mut crate::graphics_engine::graphics_engine::HContext_t,
                setup: *mut crate::graphics_engine::graphics_engine::HRenderSetup_t,
                first: u32,
                last: u32,
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
