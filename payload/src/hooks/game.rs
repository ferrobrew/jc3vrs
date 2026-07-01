use std::sync::atomic::Ordering;

use detours_macro::detour;
use jc3gi::{
    clock::Clock,
    cpu_fragment::{CpuFragmentWaitUntilSignalIsNonZero, CpuPrimaryCount},
    game::{Game, GameState, UpdateContexts},
    graphics_engine::{
        gi::{GISolver, LightManager},
        graphics_engine::{GraphicsEngine, RenderFrameCounters, get_render_frame_counters},
        render_engine::RenderEngine,
        render_pass::get_current_add_buffer,
    },
};
use re_utilities::hook_library::HookLibrary;

use crate::{
    crash::Phase,
    debug::trace::{TraceEvent, TraceState},
    stereo::STEREO_STATE,
};

use super::graphics_engine::graphics_engine::BLOCK_FLIP;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&GAME_UPDATE_BINDER)
        // CGame::Update + 0x787: nop the `m_UpdateFlags & 4` check (the jz), we will _always_ be UpdateRender-ing.
        .with_patch(Game::Update_ADDRESS + 0x787, &[0x90; 2])
        .with_static_binder(&GAME_UPDATE_RENDER_BINDER)
        // CGame::Update + 0x7A2: nop everything between UpdateRender and ++this->m_RenderCount;
        // we'll be doing that ourselves!
        .with_patch(Game::Update_ADDRESS + 0x7A2, &[0x90; 0x3A])
}

#[detour(address = jc3gi::game::Game::Update_ADDRESS)]
fn game_update(game: *const Game) -> bool {
    // Start of a real frame: re-arm the once-per-frame CClock::Update gate.
    super::clock::UPDATED_THIS_FRAME.store(false, Ordering::Relaxed);
    crate::update();
    GAME_UPDATE.get().unwrap().call(game)
}

#[detour(address = jc3gi::game::Game::UpdateRender_ADDRESS)]
fn game_update_render(game: *mut Game, update_contexts: *mut UpdateContexts) {
    unsafe {
        crate::crash::mark(Phase::UpdateRenderEnter);
        let spf = Clock::get().unwrap().GetSPF(false).min(0.5);

        crate::crash::mark(Phase::OriginalUpdateRender);
        GAME_UPDATE_RENDER
            .get()
            .unwrap()
            .call(game, update_contexts);
        GameState::PostUpdateRender(update_contexts);

        let game = game.as_mut().unwrap();

        // Start of a frame: publish the master toggle (and restore_frame_counters) from config into
        // the live stereo state, which the render hooks read via `crate::stereo`. Copy the config
        // values out and drop the lock before driving the eye loop / engine work.
        let (stereo, restore_counters, present_eye_0, restore_cb_ring, restore_ssao, restore_gi) =
            crate::config::Config::lock_query(|c| {
                (
                    c.stereo.enabled,
                    c.stereo.restore_frame_counters,
                    c.stereo.present_eye_0,
                    c.stereo.restore_cb_ring,
                    c.stereo.restore_ssao_history,
                    c.stereo.restore_gi_cascade,
                )
            });
        STEREO_STATE.lock().active = stereo;

        if stereo {
            crate::crash::mark(Phase::Eye0Snapshot);
            // Snapshot the reflection-proxy depth-history before eye 0 and restore it before eye 1,
            // so both dispatches make the same per-slot decisions -- the state then advances once
            // per real frame instead of once per dispatch, otherwise water reflections flicker.
            let effect_info = snapshot_effect_info();
            // Snapshot the prologue frame counters so we can rewind them before eye 1, keeping both
            // eyes on the same jitter phase / shadow parity / CB ring (see RESTORE_FRAME_COUNTERS).
            let frame_counters = restore_counters.then(snapshot_frame_counters);
            // Snapshot the RenderEngine per-Draw constant-buffer ring index so we can rewind it before
            // eye 1: it advances once per Draw and is not part of the frame counters above, so without
            // this the two eyes select different CB pool slots (see RESTORE_CB_RING).
            let cb_ring = restore_cb_ring.then(snapshot_cb_ring).flatten();
            // Snapshot the SSAO temporal history index and the GI cascade index so eye 1 resolves
            // against the same SSAO slot and refreshes the same LPV cascade as eye 0 -- the two passes
            // that carry an unsynchronized per-eye history (see RESTORE_SSAO_HISTORY / RESTORE_GI_CASCADE).
            let ssao_history = restore_ssao.then(snapshot_ssao_history).flatten();
            let gi_cascade = restore_gi.then(snapshot_gi_cascade).flatten();
            // Diagnostic (only while a trace is collecting, so it costs nothing in normal play): confirm
            // the between-eye restores are actually reaching live state -- a `None`/null here means the
            // snapshot missed and the restore is a silent no-op.
            if crate::debug::trace::tracing_active() {
                tracing::info!(
                    target: "stereo",
                    "restore diag: ssao_ptr_null={} ssao_snap={:?} gi_snap={:?}",
                    super::graphics_engine::ssao::ssao_pass().is_null(),
                    ssao_history,
                    gi_cascade,
                );
            }
            // Snapshot the add/draw list parity so eye 1's CKeep1000Frames toggles it to the same
            // value as eye 0, making SaveRenderFrameData set the same list pointers. This replaces the
            // former RotateRenderFrameData eye-1 gate -- the function now runs on both eyes, so the
            // overflow list is processed and the external render camera is updated on both eyes too.
            let saved_add_buffer = *get_current_add_buffer();

            // The per-eye camera offset is injected on the render camera in the SetupRenderCamera
            // hook (see hooks::camera and docs/rendering.md section 2); here we just drive the two
            // dispatches and tag each with its eye index via STEREO_STATE.draw_index. present_eye_0
            // picks which eye reaches the screen (the other's flip is blocked), so each eye can be
            // compared live.
            let present_eye = usize::from(!present_eye_0);
            TraceState::record(TraceEvent::FrameBegin {
                stereo: true,
                present_eye: Some(present_eye),
                restore_counters: Some(restore_counters),
            });

            super::draw_count::DRAW_COUNTS.clear();
            TraceState::record(TraceEvent::DrawBegin { eye: 0 });
            STEREO_STATE.lock().draw_index = 0;
            BLOCK_FLIP.store(present_eye != 0, Ordering::Relaxed);
            tracing::trace!(target: "frameloop", "game_update_render: eye 0 Draw");
            crate::crash::mark(Phase::Eye0Draw);
            game.Draw(spf);
            tracing::trace!(target: "frameloop", "game_update_render: eye 0 WaitForCPUDrawToFinish");
            crate::crash::mark(Phase::Eye0Drain);
            if let Some(ge) = GraphicsEngine::get() {
                ge.WaitForCPUDrawToFinish();
                drain_draw_thread_fragment(ge);
            }
            tracing::trace!(target: "frameloop", "game_update_render: eye 0 done");
            crate::crash::mark(Phase::Eye0Post);
            crate::debug::rt_hash::hash_engine_rts();
            crate::debug::camera::capture_render_camera(0);
            TraceState::record(TraceEvent::DrawEnd {
                eye: 0,
                counts: super::draw_count::DRAW_COUNTS.snapshot(),
            });

            crate::crash::mark(Phase::BetweenEyesRestore);
            if let Some(state) = &effect_info {
                restore_effect_info(state);
            }
            if let Some(counters) = frame_counters {
                restore_frame_counters(counters);
            }
            if let Some(ring) = cb_ring {
                restore_cb_ring_index(ring);
            }
            if let Some(history) = ssao_history {
                restore_ssao_history_indices(history);
            }
            if let Some(cascade) = gi_cascade {
                restore_gi_cascade_index(cascade);
            }
            // Restore the add/draw parity so eye 1's CKeep1000Frames produces the same toggle as eye 0,
            // and SaveRenderFrameData zeroes the same add-list (removing eye 0's draw-time additions
            // like SSAO/post blocks). This replaces the former reset_per_eye() call.
            *get_current_add_buffer() = saved_add_buffer;

            super::draw_count::DRAW_COUNTS.clear();
            TraceState::record(TraceEvent::DrawBegin { eye: 1 });
            STEREO_STATE.lock().draw_index = 1;
            BLOCK_FLIP.store(present_eye != 1, Ordering::Relaxed);
            tracing::trace!(target: "frameloop", "game_update_render: eye 1 Draw");
            crate::crash::mark(Phase::Eye1Draw);
            game.Draw(spf);
            tracing::trace!(target: "frameloop", "game_update_render: eye 1 WaitForCPUDrawToFinish");
            crate::crash::mark(Phase::Eye1Drain);
            if let Some(ge) = GraphicsEngine::get() {
                ge.WaitForCPUDrawToFinish();
                drain_draw_thread_fragment(ge);
            }
            tracing::trace!(target: "frameloop", "game_update_render: eye 1 done");
            crate::crash::mark(Phase::Eye1Post);
            crate::debug::rt_hash::hash_engine_rts();
            crate::debug::camera::capture_render_camera(1);
            TraceState::record(TraceEvent::DrawEnd {
                eye: 1,
                counts: super::draw_count::DRAW_COUNTS.snapshot(),
            });
            TraceState::end_frame();

            STEREO_STATE.lock().draw_index = 0;
        } else {
            TraceState::record(TraceEvent::FrameBegin {
                stereo: false,
                present_eye: None,
                restore_counters: None,
            });
            STEREO_STATE.lock().draw_index = 0;
            crate::crash::mark(Phase::NonStereoDraw);
            game.Draw(spf);
            crate::debug::camera::capture_render_camera(0);
            TraceState::end_frame();
        }

        // Drive the F10 stereo capture composite after the frame's draws are done (both eyes
        // captured in stereo, eye 0 in non-stereo). No-op when capture is inactive.
        crate::crash::mark(Phase::Present);
        crate::capture::present_frame();
        crate::crash::mark(Phase::FrameEnd);
    }
}

/// Snapshot of the reflection-proxy depth-history lifecycle (the 5 slot counters + the picked
/// index). Advanced once per scene dispatch, so we restore it between the two stereo Draws.
struct EffectInfoState {
    frame_index: [u8; 5],
    index: u32,
}

fn snapshot_effect_info() -> Option<EffectInfoState> {
    unsafe {
        let ge = GraphicsEngine::get()?;
        let mut frame_index = [0u8; 5];
        for (dst, slot) in frame_index.iter_mut().zip(ge.m_EffectInfo.iter()) {
            *dst = slot.m_FrameIndex;
        }
        Some(EffectInfoState {
            frame_index,
            index: ge.m_EffectInfoIndex,
        })
    }
}

fn restore_effect_info(state: &EffectInfoState) {
    unsafe {
        let Some(ge) = GraphicsEngine::get() else {
            return;
        };
        for (src, slot) in state.frame_index.iter().zip(ge.m_EffectInfo.iter_mut()) {
            slot.m_FrameIndex = *src;
        }
        ge.m_EffectInfoIndex = state.index;
    }
}

/// Wait for the engine's draw-dispatch CPU fragment to finish, the drain `WaitForCPUDrawToFinish`
/// omits. `DispatchDraw` runs the render passes on an async fragment signalled at
/// [`GraphicsEngine::m_DrawThreadWorkSignal`], and the engine itself only waits on it at the next
/// `Draw`'s entry (gated by `CpuPrimaryCount() > 1`). In stereo we mutate the shared render-frame state
/// between the eyes' Draws, so eye 0's fragment must be drained here first -- otherwise it reads a torn
/// per-camera context and faults (the intermittent open-world crash). Mirrors the engine's own guard so
/// it cannot spin on a build that draws inline.
unsafe fn drain_draw_thread_fragment(ge: &mut GraphicsEngine) {
    if !crate::config::Config::lock_query(|c| c.stereo.drain_draw_fragment) {
        return;
    }
    unsafe {
        if CpuPrimaryCount() <= 1 {
            return;
        }
        CpuFragmentWaitUntilSignalIsNonZero(&raw const ge.m_DrawThreadWorkSignal);
    }
}

fn snapshot_cb_ring() -> Option<u32> {
    unsafe { Some(RenderEngine::get()?.m_ConstantBufferRingIndex) }
}

fn restore_cb_ring_index(saved: u32) {
    unsafe {
        if let Some(re) = RenderEngine::get() {
            re.m_ConstantBufferRingIndex = saved;
        }
    }
}

fn snapshot_ssao_history() -> Option<(u32, u32)> {
    let pass = super::graphics_engine::ssao::ssao_pass();
    unsafe {
        pass.as_ref()
            .map(|pass| (pass.m_PrevFrameIndex, pass.m_CurrFrameIndex))
    }
}

fn restore_ssao_history_indices((prev, curr): (u32, u32)) {
    let pass = super::graphics_engine::ssao::ssao_pass();
    unsafe {
        if let Some(pass) = pass.as_mut() {
            pass.m_PrevFrameIndex = prev;
            pass.m_CurrFrameIndex = curr;
        }
    }
}

/// The GI solver reached through the `LightManager` singleton (`m_GIPass` -> `m_pGISolver`), or `None`
/// while GI is uninitialized for the first frames after load (every hop is null-guarded).
unsafe fn gi_solver() -> Option<*mut GISolver> {
    unsafe {
        let gi_pass = LightManager::get()?.m_GIPass.as_ref()?;
        let solver = gi_pass.m_pGISolver;
        (!solver.is_null()).then_some(solver)
    }
}

fn snapshot_gi_cascade() -> Option<u32> {
    unsafe { gi_solver().map(|s| (*s).m_CascadeToUpdate) }
}

fn restore_gi_cascade_index(saved: u32) {
    unsafe {
        if let Some(s) = gi_solver() {
            (*s).m_CascadeToUpdate = saved;
        }
    }
}

fn snapshot_frame_counters() -> RenderFrameCounters {
    unsafe { *get_render_frame_counters() }
}

fn restore_frame_counters(saved: RenderFrameCounters) {
    unsafe {
        *get_render_frame_counters() = saved;
    }
}
