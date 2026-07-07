use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use detours_macro::detour;
use jc3gi::{
    clock::Clock,
    cpu_fragment::{CpuFragmentWaitUntilSignalIsNonZero, CpuPrimaryCount},
    game::{Game, GameState, UpdateContexts},
    graphics_engine::{
        gi::{GISolver, LightManager},
        graphics_engine::{GraphicsEngine, RenderFrameCounters, get_render_frame_counters},
        render_block::{RenderBlockTypeTerrain, RenderBlockTypeTerrainPatch},
        render_engine::RenderEngine,
        render_pass::get_current_add_buffer,
    },
};
use parking_lot::Mutex;
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

        // Apply the sun-shadow diagnostic override before the original runs, so this frame's
        // sim-side CShadowManager::UpdateRender sees it and drives the engine's own SetEnabled path.
        apply_sun_shadow_override(crate::config::Config::lock_query(|c| {
            c.stereo.disable_sun_shadows
        }));

        // Apply a requested shader reload here, on the game thread before this frame's draws, so the
        // PCF-patch hook re-creates the already-loaded shaders (injection is normally after the game
        // has built them).
        super::graphics_engine::shader::process_reload_request();

        // Execute queued Scaleform debug operations (display-tree dump, clip visibility) here on
        // the game thread, which is the Scaleform capture thread.
        crate::hud::scaleform::process_requests();

        // New game frame: the next grapple-reticle projection is the frame's aim position.
        super::ui::begin_frame_aim_recording();

        // The vehicle-attach state drives the HUD's near shift; it reads game-thread animation
        // state, so it is polled here and read by the render side.
        crate::hud::depth::poll_vehicle_state();

        // Pump the OpenXR runtime once per frame and, if a session is running, begin the XR frame
        // before the eye Draws: `xrWaitFrame` inside `frame_begin` paces the app against the
        // compositor, replacing vsync (which stays suppressed via BLOCK_FLIP). Set the headpose
        // source before the original UpdateRender runs the input tick, so the sim yields to the VR
        // pose while a session is live. `frame_begin` holds the OpenXR runtime lock for the frame,
        // so nothing on the game thread may re-enter the runtime until the frame is submitted --
        // the per-eye render parameters flow through a separate slot (`vr::render_params`).
        let vr_running = crate::vr::update();
        crate::headpose::set_source(if vr_running {
            crate::headpose::Source::Vr
        } else {
            crate::headpose::Source::Sim
        });

        // Drive per-eye native render resolution before `frame_begin` (which holds the VR runtime
        // lock for the frame) and before the eye loop: this only populates the engine's deferred
        // display-mode state, which its own `HandleModeChange` services in the first eye's `Draw`
        // prologue (previous dispatch drained, this frame not yet dispatched -- the idle-context
        // boundary `ApplyResize` needs). Must sit before the first `game.Draw`.
        crate::vr::apply_native_resolution();

        let mut vr_frame = vr_running.then(crate::vr::frame_begin).flatten();

        crate::crash::mark(Phase::OriginalUpdateRender);
        GAME_UPDATE_RENDER
            .get()
            .unwrap()
            .call(game, update_contexts);
        GameState::PostUpdateRender(update_contexts);

        // Now that this frame's animation (and the head-bone anchor) is up to date, publish the VR
        // head pose and the per-eye camera parameters from the located views. When the runtime asks
        // to skip rendering, clear the parameters so the camera hook falls back to flatscreen stereo
        // for the (non-submitted) keep-alive Draws.
        let vr_cfg = crate::config::Config::lock_query(|c| c.vr.clone());
        match vr_frame.as_ref() {
            Some(frame) if frame.should_render() => crate::vr::begin_render_frame(frame, &vr_cfg),
            _ => crate::vr::clear_render_params(),
        }

        let game = game.as_mut().unwrap();

        // Start of a frame: publish the master toggle (and restore_frame_counters) from config into
        // the live stereo state, which the render hooks read via `crate::stereo`. Copy the config
        // values out and drop the lock before driving the eye loop / engine work.
        let (
            stereo,
            restore_counters,
            present_eye_0,
            restore_cb_ring,
            restore_ssao,
            restore_gi,
            invalidate_terrain_cb,
        ) = crate::config::Config::lock_query(|c| {
            (
                c.stereo.enabled,
                c.stereo.restore_frame_counters,
                c.stereo.present_eye_0,
                c.stereo.restore_cb_ring,
                c.stereo.restore_ssao_history,
                c.stereo.restore_gi_cascade,
                c.stereo.invalidate_terrain_cb,
            )
        });
        // A running VR session always renders both eyes (the XR swapchain has a slice per eye), so
        // force the stereo double-Draw on regardless of the flatscreen stereo toggle.
        let stereo = stereo || vr_running;
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
            // hook (see hooks::camera and docs/engine/rendering.md section 2); here we just drive the two
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
            super::graphics_engine::post_effects::reset_post_block_gate();
            TraceState::record(TraceEvent::DrawBegin { eye: 0 });
            STEREO_STATE.lock().draw_index = 0;
            // While a VR session runs there is no desktop present at all (the compositor presents),
            // so block the flip for both eyes; otherwise `present_eye_0` picks which eye reaches the
            // game window.
            BLOCK_FLIP.store(vr_running || present_eye != 0, Ordering::Relaxed);
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
            // Now that the frame counters are pinned back to eye 0's values, force the terrain
            // tessellation blocks to re-upload their per-slot constant buffers for eye 1: they cache
            // the baked (per-eye off-axis) view-projection keyed on the render frame number, which the
            // restore above just made identical to eye 0's, so eye 1 would otherwise reuse eye 0's
            // projection for the distant tessellated terrain (a sheared horizon wedge in VR). Only
            // meaningful while the counters are restored -- otherwise eye 1 already gets a fresh frame
            // number and re-uploads on its own.
            if restore_counters && invalidate_terrain_cb {
                invalidate_terrain_cbs();
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
            super::graphics_engine::post_effects::reset_post_block_gate();
            TraceState::record(TraceEvent::DrawBegin { eye: 1 });
            STEREO_STATE.lock().draw_index = 1;
            BLOCK_FLIP.store(vr_running || present_eye != 1, Ordering::Relaxed);
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
            super::graphics_engine::post_effects::reset_post_block_gate();
            crate::crash::mark(Phase::NonStereoDraw);
            game.Draw(spf);
            crate::debug::camera::capture_render_camera(0);
            TraceState::end_frame();
        }

        // Restore the render camera's pristine (center, unjittered) matrices now that the eye
        // dispatches are done, so the sim-side consumers that read it before the next Draw -- the
        // sun-shadow cascade fit above all -- see the engine-built state rather than the last eye's
        // jittered, offset projection (the Halton wobble otherwise flips the cascade texel snap and
        // the shadows blob-flicker; issue #10).
        restore_pristine_render_camera();

        // Submit the VR frame: blit each captured eye into its swapchain slice and end the XR frame
        // (world layer when rendered, empty otherwise). Consumes the frame context, releasing the
        // OpenXR runtime lock so the next `vr::update` can proceed.
        if let Some(frame) = vr_frame.take() {
            let should_render = frame.should_render();
            crate::vr::present_and_submit(frame, &vr_cfg);
            log_vr_frame_health(should_render);
        }

        // Drive the F10 stereo capture composite after the frame's draws are done (both eyes
        // captured in stereo, eye 0 in non-stereo). No-op when capture is inactive.
        crate::crash::mark(Phase::Present);
        crate::capture::present_frame();

        // Desktop mirror: while a session runs the engine's own present is fully blocked (BLOCK_FLIP,
        // both eyes) so the game window would freeze on a stale frame. Draw one eye into the game
        // swapchain's back buffer, letterboxed to the window aspect, composite the egui overlay, and
        // present it ourselves -- the only present this frame. This must come after the XR submit
        // (the compositor paces the loop) and after the F10 capture (a separate window/swapchain that
        // does not conflict), so the mirror never delays the HMD path. The present is unsynced by
        // mandate: a vsynced mirror on a 60 Hz monitor would throttle the 90 Hz HMD loop (see
        // `crate::vr::mirror::present_mirror`). When no session runs the engine presents normally, so
        // the mirror is skipped and flatscreen behaviour (including present_eye_0) is unchanged.
        if vr_running && vr_cfg.mirror {
            crate::vr::present_mirror(usize::from(vr_cfg.mirror_eye));
        }
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

/// Write the pristine render-camera snapshot (taken by the `SetupRenderCamera` hook before the
/// mod's jitter and eye patches) back onto the render camera, once the frame's dispatches are done.
/// The engine rebuilds the camera from the active camera during the next Draw anyway; this restore
/// covers the window in between, when the sim reads it -- most critically the sun-shadow cascade
/// fit, whose texel snapping flip-flops if it sees the jittered projection.
fn restore_pristine_render_camera() {
    if !crate::config::Config::lock_query(|c| c.stereo.restore_render_camera) {
        return;
    }
    let Some(pristine) = STEREO_STATE.lock().pristine_render_camera.take() else {
        return;
    };
    // SAFETY: the address check ensures a stale snapshot is never written onto a reallocated
    // engine.
    unsafe {
        let Some(ge) = GraphicsEngine::get() else {
            return;
        };
        let camera = &raw mut ge.m_RenderCamera;
        if camera as usize != pristine.camera {
            return;
        }
        let camera = &mut *camera;
        camera.m_Projection.data = pristine.matrices[0];
        camera.m_ProjectionF.data = pristine.matrices[1];
        camera.m_View.data = pristine.matrices[2];
        camera.m_TransformF.data = pristine.matrices[3];
        camera.m_ViewProjection.data = pristine.matrices[4];
        camera.m_ViewProjectionF.data = pristine.matrices[5];
    }
}

/// The pre-override value of the shadow manager's settings-side enabled flag, captured when the
/// diagnostic engages so releasing it restores whatever the game's own settings had. `u8::MAX` =
/// no override active.
static SAVED_SHADOWS_ENABLED: std::sync::atomic::AtomicU8 =
    std::sync::atomic::AtomicU8::new(u8::MAX);

/// Force the sun-shadow system off (or restore it) through the engine's own settings path:
/// `ShadowManager::m_Enabled`, which the sim-side `UpdateRender` syncs the engine to via
/// `SetEnabled` -- the same route the graphics menu takes, so resources are torn down and recreated
/// cleanly.
fn apply_sun_shadow_override(disable: bool) {
    // SAFETY: the graphics-engine singleton and its shadow manager are live once the engine is
    // initialised; both are null-checked, and the flag is a plain settings toggle.
    unsafe {
        let Some(manager) = GraphicsEngine::get().and_then(|ge| ge.m_ShadowManager.as_mut()) else {
            return;
        };
        let saved = SAVED_SHADOWS_ENABLED.load(Ordering::Relaxed);
        if disable && saved == u8::MAX {
            SAVED_SHADOWS_ENABLED.store(u8::from(manager.m_Enabled), Ordering::Relaxed);
            manager.m_Enabled = false;
        } else if !disable && saved != u8::MAX {
            manager.m_Enabled = saved != 0;
            SAVED_SHADOWS_ENABLED.store(u8::MAX, Ordering::Relaxed);
        }
    }
}

/// Emit a VR frame-loop health line about once every [`VR_HEALTH_INTERVAL`]: the mean submitted
/// frame time over the window and the fraction of frames the runtime asked to render. Logs are the
/// only diagnostics a headset playtest has, so this stays cheap and steady rather than per-frame.
fn log_vr_frame_health(should_render: bool) {
    let mut health = VR_FRAME_HEALTH.lock();
    let now = Instant::now();
    health.frames += 1;
    if should_render {
        health.rendered += 1;
    }
    let last = *health.last_log.get_or_insert(now);
    let elapsed = now.duration_since(last);
    if elapsed < VR_HEALTH_INTERVAL {
        return;
    }
    let frames = health.frames.max(1);
    let mean_frame_ms = elapsed.as_secs_f32() * 1000.0 / frames as f32;
    tracing::info!(
        target: "vr",
        frames,
        rendered = health.rendered,
        mean_frame_ms,
        "VR frame-loop health",
    );
    *health = VrFrameHealth {
        last_log: Some(now),
        frames: 0,
        rendered: 0,
    };
}

/// Rolling counters for [`log_vr_frame_health`].
struct VrFrameHealth {
    last_log: Option<Instant>,
    frames: u32,
    rendered: u32,
}

static VR_FRAME_HEALTH: Mutex<VrFrameHealth> = Mutex::new(VrFrameHealth {
    last_log: None,
    frames: 0,
    rendered: 0,
});

/// How often [`log_vr_frame_health`] emits a health line.
const VR_HEALTH_INTERVAL: Duration = Duration::from_secs(5);

fn snapshot_frame_counters() -> RenderFrameCounters {
    unsafe { *get_render_frame_counters() }
}

/// Force the terrain tessellation blocks to re-upload their per-slot constant buffers on the next
/// eye, by stamping every cache slot with a frame number that cannot match the current one. The
/// blocks cache the baked (per-eye off-axis) view-projection keyed on `m_RenderFrameNo`; the previous
/// frame's index is guaranteed to differ from any live frame's, so it invalidates every slot. See
/// `stereo.invalidate_terrain_cb`.
fn invalidate_terrain_cbs() {
    unsafe {
        let sentinel = get_render_frame_counters().m_FrameIndex.wrapping_sub(1);
        if let Some(terrain) = RenderBlockTypeTerrain::get() {
            terrain.m_WasCBApplied.fill(sentinel);
        }
        if let Some(patch) = RenderBlockTypeTerrainPatch::get() {
            patch.m_WasCBApplied.fill(sentinel);
        }
    }
}

fn restore_frame_counters(saved: RenderFrameCounters) {
    unsafe {
        *get_render_frame_counters() = saved;
    }
}
