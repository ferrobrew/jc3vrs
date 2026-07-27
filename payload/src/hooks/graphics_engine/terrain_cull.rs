//! Relax the volumetric-patch terrain's two *view-dependent* hull culls for stereo (issue #40).
//!
//! The tessellated terrain (`CRenderBlockTerrainPatch` -- the terrain the retail world actually
//! draws) discards whole patches inside its hull shader, from constants the type bakes once per frame
//! per constant-buffer slot in
//! [`SetupConstantBuffers`](jc3gi::graphics_engine::render_block::RenderBlockTypeTerrainPatch::SetupConstantBuffers):
//!
//! - **Back-patch cull.** The hull discards a patch when all three control-point normals are more
//!   than `m_BackPatchCullThreshold` (shipped `-0.3`, ~17.5 deg of slack) away from facing *one*
//!   direction -- the render camera's forward axis -- rather than the actual view vector to the
//!   patch. That approximation only holds for a narrow FOV: at 45-55 deg off-axis, which is most of a
//!   headset's field of view, a patch that squarely faces the eye can still be beyond the threshold
//!   from the camera axis, so it is discarded while visible.
//! - **Frustum cull.** The hull projects each control point (expanded by the patch radius) through
//!   the baked `m_OffsetViewProjection` and discards the patch when its expanded bounds fall outside
//!   a clip plane. That matrix is the *render camera's*, and the constant buffer is baked once per
//!   frame per slot -- so both eyes are culled against one frustum. Under the single-pass collapse
//!   that frustum belongs to neither eye: the render camera stays centred with the engine's own
//!   projection, built from the mod's injected 90 deg *vertical* FOV and the double-wide target's
//!   aspect (`Camera::RecalcProjection` -> `CMatrix4f::PerspectiveFov`, which takes the vertical FOV
//!   and divides by the aspect for the horizontal extent). A headset eye whose vertical FOV exceeds
//!   90 deg, or whose display cant rotates it off the centre axis, sees past that frustum's top and
//!   bottom, and every terrain patch out there is discarded for both eyes.
//!
//! A patch the hull discards is not drawn in any pass, and the coarser tile that covers the same
//! footprint is separately clipped away by the LOD clip (which keys on the streaming visibility mask,
//! not on the view), so nothing fills the hole: the pixels keep the far depth and resolve dark --
//! world-locked black patch-shaped gaps that flip with head rotation.
//!
//! The fix substitutes the two enable flags at *upload* time: the detour clears them around the
//! type's own bake, so the baked constants carry `0`, and restores the engine's values immediately
//! afterwards (the engine's own settings, and its debug UI's view of them, are untouched). The LOD
//! clip and the cull-by-detail term are left alone -- neither is view-dependent. Scoped to stereo
//! frames and to [`StereoConfig::relax_terrain_patch_hull_culls`](crate::config::StereoConfig).
//!
//! Cost: the patches in the margin are tessellated and rasterized instead of dropped, bounded by
//! whatever the CPU-side patch cull already admitted (itself widened to the binocular union in
//! [`super::culling`]). Back-facing triangles still die at the rasterizer, and out-of-frustum ones at
//! the clipper.

use detours_macro::detour;
use jc3gi::graphics_engine::{
    graphics_engine::RenderContext, render_block::RenderBlockTypeTerrainPatch,
};
use re_utilities::hook_library::HookLibrary;

use crate::config::Config;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&TERRAIN_PATCH_SETUP_CONSTANT_BUFFERS_BINDER)
}

#[detour(
    address = jc3gi::graphics_engine::render_block::RenderBlockTypeTerrainPatch::SetupConstantBuffers_ADDRESS
)]
fn terrain_patch_setup_constant_buffers(
    this: *mut RenderBlockTypeTerrainPatch,
    render_context: *mut RenderContext,
) {
    let original = TERRAIN_PATCH_SETUP_CONSTANT_BUFFERS.get().unwrap();
    let relax =
        crate::stereo::active() && Config::lock_query(|c| c.stereo.relax_terrain_patch_hull_culls);
    if !relax || this.is_null() {
        original.call(this, render_context);
        return;
    }
    // SAFETY: `this` is the live terrain-patch render block type the engine passed in, on the draw
    // thread. The two flags are written through raw pointers rather than a `&mut` so nothing aliases
    // the callee's own view of the type across the trampoline call, and both are restored before
    // returning, so this changes only what the bake below writes into the hull/domain constants.
    unsafe {
        let back = &raw mut (*this).m_EnableBackPatchCulling;
        let frustum = &raw mut (*this).m_EnableFrustumPatchCulling;
        let saved = (*back, *frustum);
        *back = false;
        *frustum = false;
        original.call(this, render_context);
        *back = saved.0;
        *frustum = saved.1;
    }
}
