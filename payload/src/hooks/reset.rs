//! Per-eye reset of draw-time render-block state (the stereo "reset between eyes").
//!
//! The Draw driver runs the engine's render twice per stereo frame. The geometry fix gates
//! `RotateRenderFrameData` on eye 1, so the per-pass add-lists are never zeroed between the two
//! dispatches. Passes whose render-block-items are added *during* Draw -- SSAO and the post-effect
//! passes -- therefore append eye 1's items onto eye 0's and draw twice (the ~2x AO and the doubled
//! post chain). Geometry is populated by the sim *before* the rotation, lives in the draw-lists, and
//! is untouched by this.
//!
//! [`reset_per_eye`] zeroes every pass's `m_CurrentAddList` element count between eye 0 and eye 1 --
//! the add-list-reset half of the rotation, without the parity flip / draw-list repoint that would
//! wipe the reused geometry. It runs after eye 0's `WaitForCPUDrawToFinish`, so no worker is appending
//! concurrently. As more per-eye state proves resettable this way, fold it in here.

use jc3gi::graphics_engine::render_engine::RenderEngine;

use crate::debug::trace::{TraceEvent, TraceState};

/// Zero every render pass's draw-time add-list before the second eye renders, so eye 1 re-adds its
/// SSAO / post-effect blocks fresh instead of accumulating onto eye 0's.
pub(super) fn reset_per_eye() {
    // SAFETY: reads the render-engine singleton pointer; valid once the engine is initialised.
    let Some(re) = (unsafe { RenderEngine::get() }) else {
        return;
    };
    let mut cleared = 0u32;
    for list in re.m_RenderPasses.iter() {
        let (mut p, end) = (list.begin, list.end);
        while !p.is_null() && p < end {
            // SAFETY: `p` walks the pass vector's [begin, end); each `*p` is a live CRenderPass*, and
            // eye 0's workers have already drained (WaitForCPUDrawToFinish), so nothing else writes
            // these lists right now.
            unsafe {
                if let Some(pass) = (*p).as_mut()
                    && let Some(add) = pass.m_CurrentAddList.as_mut()
                {
                    cleared += u32::from(add.m_NumElements != 0);
                    add.m_NumElements = 0;
                }
                p = p.add(1);
            }
        }
    }
    TraceState::record(TraceEvent::ResetPerEye { cleared });
}
