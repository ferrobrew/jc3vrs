//! Detour on the SSAO pass.
//!
//! The SSAO temporal filter blends each frame's AO against the previous frame through a two-slot
//! history ping-pong advanced once per `Draw`. A stereo (twice-per-frame) render double-steps that
//! history, so each eye filters against the other eye's AO and the occlusion compounds. Clearing
//! `m_EnableTemporalFilter` while stereo is active computes AO fresh per eye instead.

use detours_macro::detour;
use jc3gi::graphics_engine::ssao::SSAOPass;
use re_utilities::hook_library::HookLibrary;

use crate::{
    config::Config,
    debug::trace::{TraceEvent, TraceState},
    stereo,
};

pub(super) fn extend(library: HookLibrary) -> HookLibrary {
    library.with_static_binder(&SSAO_DRAW_BINDER)
}

// SSAOPass::Draw -- the per-dispatch SSAO pass. Its temporal filter advances a per-frame AO history,
// so a twice-per-frame stereo render double-steps it and each eye filters against the other eye's AO.
// Disable the filter while stereo is active (AO is then computed fresh per eye); otherwise restore the
// engine default so non-stereo keeps temporal denoising.
#[detour(address = jc3gi::graphics_engine::ssao::SSAOPass::Draw_ADDRESS)]
fn ssao_draw(this: *mut SSAOPass) {
    let disabled = stereo::active() && Config::lock_query(|c| c.stereo.disable_ssao_temporal);
    if let Some(pass) = unsafe { this.as_mut() } {
        pass.m_EnableTemporalFilter = !disabled;
    }
    TraceState::record_eye(TraceEvent::SsaoDraw {
        temporal_disabled: disabled,
    });
    SSAO_DRAW.get().unwrap().call(this);
}
