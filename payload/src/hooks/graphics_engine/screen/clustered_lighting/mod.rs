//! The clustered-lighting froxel tile-bounds fix for off-axis VR projections (issue #35), and the
//! per-eye froxel grid under the single-pass collapse.
//!
//! `CRenderBlockDeferredLighting::DrawClustered` reconstructs a symmetric frustum from the vertical
//! FOV and aspect ratio, then uploads 8 floats (2 vec4s) to fragment constant buffer 1 (cb1) as
//! per-tile frustum edge bounds. The formula `boundX(i) = horiz * (1 - 2*i/tileCountX)` is always
//! centered on the optical axis and cannot encode the off-axis shift that VR per-eye projections
//! introduce — so lights are assigned to the wrong 64-pixel tiles, producing blocky, screen-aligned
//! lighting artifacts in VR.
//!
//! The geometry proxy transform (cb0, uploaded earlier in `DrawClustered`) is built from
//! `RenderContext::m_ProjectionF`, which already carries the off-axis projection (written by the
//! camera hook before `SetupRenderCamera`). So cb0 is correct outside the collapse; only cb1 needs
//! overriding there.
//!
//! Because the cb1 upload and the light-proxy draws both happen inside `DrawClustered`, we cannot
//! re-upload after the original returns. Instead, a thread-local flag is set around the original
//! `DrawClustered` call, and a detour on `Graphics::SetFragmentProgramConstants` intercepts the cb1
//! upload (identified by `cb_index=1, start_offset=0, count=2`) and replaces the data with
//! off-axis-derived values computed from the per-eye projection matrix. The second
//! `SetFragmentProgramConstants` call in `DrawClustered` (tile grid dimensions, `count=1`) is not
//! intercepted.
//!
//! Both `DrawClustered` and its `SetFragmentProgramConstants` calls run on the render thread, so the
//! thread-local flag correctly scopes the interception.
//!
//! # The per-eye resolve under the single-pass collapse
//!
//! `DrawClustered` ends with the deferred resolve: a fullscreen triangle whose vertex shader builds a
//! view ray from the `ViewProjInv` the [`crate::hooks::graphics_engine::reconstruction`] detour substitutes, and whose pixel
//! shader samples the sun-shadow cascade over the world positions that ray reconstructs. Under the
//! collapse that one draw spans **both** eye halves of the double-wide target while the substituted
//! basis describes one eye's frustum, so neither half reconstructs correctly and the error rotates
//! with the camera -- the sun shadows slide across the screen instead of staying on the world.
//!
//! Under [`StereoConfig::single_pass_reconstruct_per_eye`](crate::stereo::config::StereoConfig), the block's
//! whole `Draw` is re-issued once per eye through
//! [`split_fullscreen_pass`](crate::hooks::graphics_engine::reconstruction::split_fullscreen_pass), which masks
//! each run to that eye's half and hands it that eye's basis. The re-issue is of the whole block, not of
//! the resolve alone, because the resolve is not separately reachable: the mask has to be armed after
//! the block's last render-setup bind, and the block's own `PerspectiveFovInverse` call is the only seam
//! that sits between that bind and the draw. The froxel split below needs to know when the resolve could
//! not be masked at all, because that is exactly when it must decline rather than leave the grid
//! half-built -- `split_fullscreen_pass` reports that as
//! [`SplitOutcome::Demoted`](crate::hooks::graphics_engine::reconstruction::SplitOutcome::Demoted).
//!
//! # The per-eye froxel grid
//!
//! The block's earlier light-assignment phase therefore runs twice as well, and by default both runs
//! build the *same* grid: one eye's projection paired with the **double-wide** tile count, so the grid
//! is twice as wide as the frustum it describes and every local light lands in the wrong tiles. That
//! is wrong for both eyes, and it is not confined to this block -- ~20 forward-lit render-block types
//! bind the grid through `CLightManager::SetupForwardLightingResources`, and foliage reads it a whole
//! frame early, in `RP_VEGETATION_OPAQUE`.
//!
//! [`StereoConfig::single_pass_clustered_per_eye`](crate::stereo::config::StereoConfig) makes the assignment
//! per-eye too. Per run (see `docs/engine/rendering/lighting-shadow-pipeline.md` section 4.1 for how the engine
//! builds the grid, and `docs/mod/stereo/single-pass-stereo.md` for the split):
//!
//! 1. the light-assignment target's viewport is narrowed to this eye's half of the *tile* grid, which
//!    is a different half-width from the framebuffer's, so the collapse's own viewport helpers cannot
//!    be reused;
//! 2. the geometry transform (cb0) is rebuilt from this eye's projection;
//! 3. the tile bounds (cb1) are made affine in the **absolute** tile index, since the pixel shader
//!    derives its frustum from `SV_Position`, which includes the viewport's `TopLeftX`;
//! 4. and the second run's `Graphics::Clear` is suppressed.
//!
//! The two halves then compose: the assignment phase blends per tile with a commutative equation, the
//! eyes' tile halves are disjoint, and the compaction phase is per-tile-local. So the grid ends the
//! frame valid in **both** halves, which is what the forward consumers (this frame and next) need --
//! and the whole reason the clear has to be suppressed rather than the assignment simply repeated.
//!
//! Everything the split touches is a constant-buffer upload or viewport/clear state the mod already
//! intercepts; nothing is written into engine memory. It declines itself, leaving the un-split
//! behaviour intact, whenever the eye seam would not fall on a whole tile column or the bound
//! assignment target is not the grid it sized for.

use re_utilities::hook_library::HookLibrary;

mod bounds;
mod draw;
mod split;
mod uploads;

pub(crate) use split::{on_render_setup_bound, substitute_assignment_view, suppress_clear};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_hook_library(draw::hook_library())
        .with_hook_library(uploads::hook_library())
}
