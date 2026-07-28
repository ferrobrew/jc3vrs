//! The atmospheric-scattering / aerial-perspective pass, run once per eye under the single-pass
//! collapse.
//!
//! The pass reconstructs world position from depth for the *whole* screen -- sky included -- and then
//! ray-marches the sun shadow cascade and aerial perspective over the reconstructed positions. That
//! makes it the second consumer of the [`super::reconstruction`] basis after the deferred resolve,
//! and it fails under the collapse the same way: one draw spans both eye halves of the double-wide
//! target while the substituted inverse describes one eye's frustum, so neither half reconstructs
//! correctly and the error turns with the camera.
//!
//! The visible consequence is that the sun shadow keeps sliding across the world even with the
//! deferred resolve already split per eye ([`super::clustered_lighting`]) -- the cascade is sampled in
//! both passes, and fixing only one leaves the other painting the mistake back over it.
//!
//! The fix is the shared one: re-issue the block's `Draw` once per eye inside a
//! [`reconstruction::split_fullscreen_pass`] scope, which masks each run to that eye's half and hands
//! it that eye's basis. Everything the split touches is scissor state and a constant the mod already
//! substitutes; nothing is written into engine memory, and it declines to the un-split pass whenever
//! the preconditions do not hold.

use detours_macro::detour;
use jc3gi::graphics_engine::{
    graphics_engine::RenderContext,
    render_block::{RBIInfo, RenderBlockAtmosphericScattering},
};
use re_utilities::hook_library::HookLibrary;

use super::reconstruction;
use crate::config::Config;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&ATMOSPHERIC_SCATTERING_DRAW_BINDER)
}

#[detour(
    address = jc3gi::graphics_engine::render_block::RenderBlockAtmosphericScattering::Draw_ADDRESS
)]
fn atmospheric_scattering_draw(
    this: *mut RenderBlockAtmosphericScattering,
    rc: *mut RenderContext,
    info: *const RBIInfo,
) {
    let enabled = Config::lock_query(|c| {
        c.stereo.single_pass_atmospheric_per_eye && c.stereo.reconstruct_offaxis_inverse
    });
    let call = || {
        ATMOSPHERIC_SCATTERING_DRAW
            .get()
            .unwrap()
            .call(this, rc, info)
    };

    // SAFETY: `rc` is the live render context for this dispatch; the caller (the engine's draw
    // dispatch) guarantees it is valid for the duration of `Draw`.
    let ctx = unsafe { rc.as_ref() }.map(|rc| rc.m_Context);
    if reconstruction::split_fullscreen_pass(enabled, ctx, |_eye| call())
        == reconstruction::SplitOutcome::NotTaken
    {
        call();
    }
}
