#![cfg_attr(any(), rustfmt::skip)]
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
