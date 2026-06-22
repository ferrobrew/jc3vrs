//! Detour on the SSAO pass.
//!
//! The SSAO history index advances once per `CRenderBlockSSAO::Draw` (an inlined
//! `SetNextHistoryBuffer` at the end of the apply), not in `RotateRenderFrameData`. A stereo render
//! dispatches that draw twice per frame, so the two-slot AO history double-steps and each eye's
//! temporal filter blends against the other eye's AO. Clearing `m_EnableTemporalFilter` would avoid
//! that, but the render block's final composite draw lives inside that flag's branch, so doing so
//! disables SSAO entirely.
//!
//! The engine's own "no valid history" lever is `m_FirstPass`: on, it skips the history resolve and
//! weights the temporal shader fully toward the current AO. The render block clears it after the first
//! apply; [`ssao_draw`] forces it back on before each stereo eye so every dispatch computes AO fresh
//! from its own depth, with the apply still running. Each eye packs and reads its own current
//! depth-buffer slot, so the still-advancing index causes no cross-eye depth contamination.

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

// SSAOPass::Draw -- the per-dispatch SSAO pass. Its history double-steps across the two stereo
// dispatches, so force m_FirstPass on (the engine's "compute fresh, ignore history" state) for each
// eye while stereo is active. The apply still runs; only the cross-eye temporal blend is suppressed.
#[detour(address = jc3gi::graphics_engine::ssao::SSAOPass::Draw_ADDRESS)]
fn ssao_draw(this: *mut SSAOPass) {
    let force = stereo::active() && Config::lock_query(|c| c.stereo.force_ssao_first_pass);
    if let Some(pass) = unsafe { this.as_mut() } {
        pass.m_FirstPass |= force;
    }
    TraceState::record_eye(TraceEvent::SsaoDraw {
        forced_first_pass: force,
    });
    SSAO_DRAW.get().unwrap().call(this);
}
