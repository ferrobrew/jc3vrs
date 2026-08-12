//! World-geometry and culling detours: the render blocks that draw the scene, and the visibility
//! gates that decide what reaches them.

use re_utilities::hook_library::HookLibrary;

// The VR two-eye cull-frustum widening (`GetBFBCFrustumParamsForCameraAndTime`).
pub(crate) mod culling;
// `render_block` is crate-visible so hooks::character can publish the facial classification bones.
pub(crate) mod render_block;
// Diagnostic override of the base VolumetricTerrain color-pass hull-clip type (black cliff-wall
// tiles).
pub mod terrain;
// The stereo relaxation of the volumetric-patch terrain's view-dependent hull culls (black terrain
// patch gaps).
pub(crate) mod terrain_cull;
/// Bundle the world-geometry detours into one hook library.
pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(render_block::hook_library())
        .with_hook_library(culling::hook_library())
        .with_hook_library(terrain::hook_library())
        .with_hook_library(terrain_cull::hook_library())
}
