#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The argument block passed to a patch map's per-patch CPU update fragment. Partial: only the
/// fields the base-terrain fragment ([`TerrainPatchUpdate`]) reads are mapped; `0x30` holds the
/// patch map's type context (for the terrain maps, the render-pass pointer table the fragment
/// submits into).
pub struct PatchContext {
    _field_0: [u8; 8],
    /// The view description for this update; see [`PatchViewDesc`].
    pub m_ViewDesc: *const crate::patch_system::PatchViewDesc,
    _field_10: [u8; 8],
    /// The patch's header; see [`PatchHeader`].
    pub m_Header: *mut crate::patch_system::PatchHeader,
    _field_20: [u8; 8],
    /// The patch's per-patch constant memory. For the base terrain maps this is the patch's
    /// `CRenderBlockTerrain` instance, constructed in place when the map was created.
    pub m_RenderBlock: *mut crate::graphics_engine::render_block::RenderBlockTerrain,
    _field_30: [u8; 32],
}
fn _PatchContext_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x50], PatchContext>([0u8; 0x50]);
    }
    unreachable!()
}
impl PatchContext {}
impl std::convert::AsRef<PatchContext> for PatchContext {
    fn as_ref(&self) -> &PatchContext {
        self
    }
}
impl std::convert::AsMut<PatchContext> for PatchContext {
    fn as_mut(&mut self) -> &mut PatchContext {
        self
    }
}
#[repr(C, align(8))]
/// A patch's bookkeeping header within a patch map (the Avalanche `patchsystem` dependency
/// library's grid-of-patches streaming, used by the landscape systems: the base VolumetricTerrain
/// patch maps `terrain_patch_lod9..12`, water, and vegetation). Partial: only the fields the
/// terrain patch-update fragment reads are mapped.
pub struct PatchHeader {
    _field_0: [u8; 16],
    /// The patch's X coordinate in its map grid. The patch's world X derives as
    /// `(m_PatchX << m_Lod) + world_origin_x`, scaled by the world context's inverse XZ scale.
    pub m_PatchX: i16,
    /// The patch's Z coordinate in its map grid; see [`m_PatchX`](PatchHeader::m_PatchX).
    pub m_PatchZ: i16,
    /// Patch lifecycle status bits: `0x1` = constructed, `0x2` = destruction pending, `0x4` =
    /// construction pending, `0x8` = destruction complete, `0x10` = construction complete, `0x20` =
    /// inside world.
    pub m_StatusBits: u16,
    _field_16: [u8; 3],
    /// A per-patch state counter the patch's update fragment advances during multi-step
    /// construction or destruction.
    pub m_UserState: u8,
    /// The patch map's LOD level (9..=12 for the base terrain maps; the patch spans
    /// `1 << m_Lod` world units per side before world scaling).
    pub m_Lod: u8,
    _field_1b: [u8; 1],
    /// Per-frustum visibility bits, written by the patch system's BFBC cull step each update: bit
    /// `i` is set when the patch's AABB passed the cull for frustum entry `i` of the update's view
    /// description ([`PatchViewDesc::m_FrustumIDs`] maps entries to frustum ids). The patch-update
    /// fragments read these to decide which render passes receive the patch's render block.
    pub m_VisibilityBits: u16,
    _field_1e: [u8; 10],
    /// The patch's BFBC occludee handle, allocated by `BFBCAdd` when the patch map is created; the
    /// patch's world AABB is pushed through `BFBCSetAABB` under this handle during construction.
    pub m_BFBCOcludee: u16,
    _field_2a: [u8; 6],
}
fn _PatchHeader_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], PatchHeader>([0u8; 0x30]);
    }
    unreachable!()
}
impl PatchHeader {}
impl std::convert::AsRef<PatchHeader> for PatchHeader {
    fn as_ref(&self) -> &PatchHeader {
        self
    }
}
impl std::convert::AsMut<PatchHeader> for PatchHeader {
    fn as_mut(&mut self) -> &mut PatchHeader {
        self
    }
}
#[repr(C, align(8))]
/// The per-update view description handed to the patch-update fragments: the set of cull frusta
/// the patch system's BFBC step ran this update. For the landscape patch system
/// (`CLandscapeManager::UpdatePatchSystem`) the entries are: entry 0 = the occluder manager's main
/// cull camera (frustum id 0), entries 1..=N = the active shadow cascades (ids 1..=8), then two
/// reflective-shadow frusta (ids 9..=10), and a final second run of the same main cull camera
/// (id 12). Partial mapping.
pub struct PatchViewDesc {
    _field_0: [u8; 48],
    /// The frustum id for each entry, indexed by the same entry index as the patch's
    /// [`PatchHeader::m_VisibilityBits`] bit.
    pub m_FrustumIDs: *const u32,
    /// The number of frustum entries this update (also the number of populated visibility bits).
    pub m_FrustumCount: u32,
    _field_3c: [u8; 4],
}
fn _PatchViewDesc_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x40], PatchViewDesc>([0u8; 0x40]);
    }
    unreachable!()
}
impl PatchViewDesc {}
impl std::convert::AsRef<PatchViewDesc> for PatchViewDesc {
    fn as_ref(&self) -> &PatchViewDesc {
        self
    }
}
impl std::convert::AsMut<PatchViewDesc> for PatchViewDesc {
    fn as_mut(&mut self) -> &mut PatchViewDesc {
        self
    }
}
pub const TerrainPatchUpdate_ADDRESS: usize = 0x1410C98A0;
/// The per-patch CPU fragment for the base VolumetricTerrain patch maps (registered against the
/// `TerrainPatchUpdate` hash by `OldTerrainPatchSystem::TerrainPatchSystemCreateLayers`). It
/// advances patch construction and destruction (creating or destroying the patch's
/// [`RenderBlockTerrain`](graphics_engine::render_block::RenderBlockTerrain) constant buffers,
/// updating the global terrain mask, and adjusting per-LOD memory statistics), and for constructed
/// patches it updates the block's alpha fade, sort id, and tessellated index count, then submits
/// the block to one render pass per set [`PatchHeader::m_VisibilityBits`] bit: frustum id 0 routes
/// to the color passes (the near-detail pass for base-LOD tiles within one tile of the camera, the
/// near pass for other tiles at the context's high-detail LOD, and the far pass otherwise), ids
/// 1..=8 to the static shadow passes (only when the terrain type renders to shadow maps), ids
/// 9..=10 to the reflective shadow passes, and id 12 to the depth prepass.
pub unsafe fn TerrainPatchUpdate(
    patch_context: *mut crate::patch_system::PatchContext,
    fragment_context: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            patch_context: *mut crate::patch_system::PatchContext,
            fragment_context: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(TerrainPatchUpdate_ADDRESS);
        f(patch_context, fragment_context)
    }
}
pub const TerrainPatchSystemFragment_ADDRESS: usize = 0x1410CAC50;
/// The per-patch CPU fragment for the volumetric-patch terrain maps (the
/// `CRenderBlockTerrainPatch` system — the terrain renderer active in the retail world; the
/// [`TerrainPatchUpdate`] maps are dormant there). Structurally parallel to [`TerrainPatchUpdate`]:
/// it advances patch construction and destruction, and for constructed patches updates the block's
/// alpha fade and tessellated index count, then submits the patch's render blocks to one render
/// pass per set [`PatchHeader::m_VisibilityBits`] bit with the same frustum-id routing (id 0 →
/// color passes, ids 1..=8 → shadow passes, ids 9..=11 → reflective shadow passes, id 12 → the
/// depth prepass). Base-LOD (9) patches within the camera's 2×2 quadrant additionally submit
/// per-quadrant sub-blocks to the near/tessellation passes.
pub unsafe fn TerrainPatchSystemFragment(
    patch_context: *mut crate::patch_system::PatchContext,
    fragment_context: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            patch_context: *mut crate::patch_system::PatchContext,
            fragment_context: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(TerrainPatchSystemFragment_ADDRESS);
        f(patch_context, fragment_context)
    }
}
