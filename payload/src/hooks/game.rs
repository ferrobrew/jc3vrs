use std::sync::atomic::Ordering;

use detours_macro::detour;
use jc3gi::{
    clock::Clock,
    game::{Game, GameState, UpdateContexts},
    graphics_engine::graphics_engine::{
        GraphicsEngine, RenderFrameCounters, get_render_frame_counters,
    },
};
use re_utilities::hook_library::HookLibrary;

use crate::TraceEvent;
use crate::stereo::STEREO_STATE;

use super::graphics::BLOCK_FLIP;

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
        let spf = Clock::get().unwrap().GetSPF(false).min(0.5);

        GAME_UPDATE_RENDER
            .get()
            .unwrap()
            .call(game, update_contexts);
        GameState::PostUpdateRender(update_contexts);

        let game = game.as_mut().unwrap();

        // Start of a frame: publish the master toggle (and restore_frame_counters) from config into
        // the live stereo state, which the render hooks read via `crate::stereo`. Copy the config
        // values out and drop the lock before driving the eye loop / engine work.
        let (stereo, restore_counters, present_eye_0) = crate::config::Config::lock_query(|c| {
            (
                c.stereo.enabled,
                c.stereo.restore_frame_counters,
                c.stereo.present_eye_0,
            )
        });
        STEREO_STATE.lock().active = stereo;

        if stereo {
            // Snapshot the reflection-proxy depth-history before eye 0 and restore it before eye 1,
            // so both dispatches make the same per-slot decisions -- the state then advances once
            // per real frame instead of once per dispatch, otherwise water reflections flicker.
            let effect_info = snapshot_effect_info();
            // Snapshot the prologue frame counters so we can rewind them before eye 1, keeping both
            // eyes on the same jitter phase / shadow parity / CB ring (see RESTORE_FRAME_COUNTERS).
            let frame_counters = restore_counters.then(snapshot_frame_counters);

            // The per-eye camera offset is injected on the render camera in the SetupRenderCamera
            // hook (see hooks::camera and docs/rendering.md section 2); here we just drive the two
            // dispatches and tag each with its eye index via STEREO_STATE.draw_index. present_eye_0
            // picks which eye reaches the screen (the other's flip is blocked), so each eye can be
            // compared live.
            let present_eye = usize::from(!present_eye_0);
            crate::trace(TraceEvent::FrameBegin {
                stereo: true,
                present_eye: Some(present_eye),
                restore_counters: Some(restore_counters),
            });

            crate::DRAW_CALLS.store(0, Ordering::Relaxed);
            crate::DRAW_INDEXED_CALLS.store(0, Ordering::Relaxed);
            crate::DISPATCH_CALLS.store(0, Ordering::Relaxed);
            crate::trace(TraceEvent::DrawBegin { eye: 0 });
            STEREO_STATE.lock().draw_index = 0;
            BLOCK_FLIP.store(present_eye != 0, Ordering::Relaxed);
            game.Draw(spf);
            if let Some(ge) = GraphicsEngine::get() {
                ge.WaitForCPUDrawToFinish();
            }
            crate::capture_render_camera(0);
            crate::trace(TraceEvent::DrawEnd {
                eye: 0,
                draw: crate::DRAW_CALLS.load(Ordering::Relaxed),
                draw_indexed: crate::DRAW_INDEXED_CALLS.load(Ordering::Relaxed),
                dispatch: crate::DISPATCH_CALLS.load(Ordering::Relaxed),
            });

            if let Some(state) = &effect_info {
                restore_effect_info(state);
            }
            if let Some(counters) = frame_counters {
                restore_frame_counters(counters);
            }

            crate::DRAW_CALLS.store(0, Ordering::Relaxed);
            crate::DRAW_INDEXED_CALLS.store(0, Ordering::Relaxed);
            crate::DISPATCH_CALLS.store(0, Ordering::Relaxed);
            crate::trace(TraceEvent::DrawBegin { eye: 1 });
            STEREO_STATE.lock().draw_index = 1;
            BLOCK_FLIP.store(present_eye != 1, Ordering::Relaxed);
            game.Draw(spf);
            if let Some(ge) = GraphicsEngine::get() {
                ge.WaitForCPUDrawToFinish();
            }
            crate::capture_render_camera(1);
            crate::trace(TraceEvent::DrawEnd {
                eye: 1,
                draw: crate::DRAW_CALLS.load(Ordering::Relaxed),
                draw_indexed: crate::DRAW_INDEXED_CALLS.load(Ordering::Relaxed),
                dispatch: crate::DISPATCH_CALLS.load(Ordering::Relaxed),
            });
            crate::trace_end_frame();

            STEREO_STATE.lock().draw_index = 0;
        } else {
            crate::trace(TraceEvent::FrameBegin {
                stereo: false,
                present_eye: None,
                restore_counters: None,
            });
            STEREO_STATE.lock().draw_index = 0;
            game.Draw(spf);
            crate::capture_render_camera(0);
            crate::trace_end_frame();
        }
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

fn snapshot_frame_counters() -> RenderFrameCounters {
    unsafe { *get_render_frame_counters() }
}

fn restore_frame_counters(saved: RenderFrameCounters) {
    unsafe {
        *get_render_frame_counters() = saved;
    }
}
