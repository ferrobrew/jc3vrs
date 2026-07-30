//! The VR runtime state singleton and the game-thread entry points that drive it: bring-up and retry,
//! the OpenXR event pump, session-state transitions, and ordered teardown. The per-frame half of the
//! state machine -- locating views, the swapchain acquire/release, and the frame submit -- lives in
//! [`crate::vr::frame_loop`].

use std::time::Instant;

use anyhow::Context as _;
use openxr as xr;
use parking_lot::Mutex;

use crate::{
    config::Config,
    hooks::graphics_engine::graphics_engine,
    vr::{
        VIEW_TYPE, VrConfig, blit,
        eye_resolution::scaled_eye_size,
        foveation,
        loader::{LoaderUnavailable, load_entry},
        mirror,
        persist::{
            acquire_instance, acquire_session, clear_persisted, persist_session, stash_instance,
        },
        recenter::Baseline,
        resolution,
        session::Session,
    },
};

/// Register the VR runtime's shutdown cleanup. Call once at init (from [`crate::initialize_from_game`]
/// via the module declaration wiring). The cleanup fully tears the runtime down so the OpenXR
/// instance never outlives the DLL on uninject → reinject.
pub fn install() {
    // Register the native-resolution shutdown restore: the deferred resize back to the pre-VR display
    // size is requested while the hooks are still live, so the delayed hook uninstall (lib.rs
    // `shutdown_startup`) leaves the `Draw` prologue time to service it before teardown.
    resolution::install();
    crate::lifecycle::on_cleanup(|_renderer| {
        blit::teardown();
        foveation::teardown();
        crate::far_field::share::teardown();
        mirror::teardown();
        uninstall();
    });
}

/// The once-per-frame entry point, called from the game thread by `hooks::game::game_update_render`.
/// Pumps OpenXR events, drives bring-up/retry/teardown per config, and
/// returns whether a session is currently running (so the caller can decide whether to submit VR
/// frames). OpenXR failures degrade to flatscreen stereo and are retried; the one exception is a
/// loader that will not load, which aborts the process (see [`LoaderUnavailable`]).
pub fn update() -> bool {
    // Once eject has begun, do nothing: the shutdown cleanup tears the runtime down (persisting the
    // instance/session handles for the next injection), and an ungated re-entry here would see
    // `instance.is_none()`, re-acquire those persisted handles, and rebuild the swapchain -- racing
    // the hook uninstall and crashing the game on uninject. Returning false also skips `frame_begin`
    // and `present_and_submit` in the caller, quiescing the whole VR frame path.
    if crate::is_shutting_down() {
        return false;
    }

    let cfg = Config::lock_query(|c| c.vr.clone());
    let mut state = VR_STATE.lock();

    if !cfg.enabled {
        if state.instance.is_some() {
            tracing::info!(target: "vr", "vr.enabled turned off; tearing down the OpenXR runtime");
            // VR is being genuinely stopped, so destroy everything rather than persisting for reuse.
            state.teardown(false);
        }
        return false;
    }

    if state.instance.is_none() {
        state.try_bring_up(&cfg);
        return state.is_running();
    }

    state.pump_events();
    state.is_running()
}

/// Whether an OpenXR session is currently running (READY..STOPPING). Cheap; locks the state briefly.
pub fn is_running() -> bool {
    VR_STATE.lock().is_running()
}

/// A snapshot of the VR runtime state for the debug UI ([`crate::ui::vr`]).
pub struct VrStatus {
    /// Whether `vr.enabled` is set (the master switch). Off leaves the mod in flatscreen stereo.
    pub enabled: bool,
    /// Whether an OpenXR instance is currently up (bring-up succeeded).
    pub instance_up: bool,
    /// Whether a session is currently running (READY..STOPPING).
    pub running: bool,
    /// The runtime name reported at bring-up, or `None` while torn down.
    pub runtime_name: Option<String>,
    /// The effective per-eye render resolution (recommended × `resolution_scale`) while a session is
    /// running, or `None` otherwise.
    pub eye_resolution: Option<(u32, u32)>,
    /// Whether the runtime state was busy and this snapshot is stale (every field but `enabled` is a
    /// carried-over default). The debug UI says so rather than pretending.
    pub busy: bool,
}

/// Snapshot the VR runtime state for the debug UI. Locks the config and the runtime state briefly.
pub fn status() -> VrStatus {
    let cfg = Config::lock_query(|c| c.vr.clone());
    // `try_lock`, never `lock`. The deferred frame tail holds this lock across its drain and submit,
    // and the debug UI runs on the game thread -- which is also what the tail waits on. Blocking here
    // puts a diagnostic panel inside that cycle, and opening the VR tab at the wrong moment wedged
    // the process outright. A stale readout for one frame is not worth a deadlock.
    let Some(state) = VR_STATE.try_lock() else {
        return VrStatus {
            enabled: cfg.enabled,
            instance_up: false,
            running: false,
            runtime_name: None,
            eye_resolution: None,
            busy: true,
        };
    };
    VrStatus {
        enabled: cfg.enabled,
        instance_up: state.instance.is_some(),
        running: state.is_running(),
        runtime_name: state.runtime_name.clone(),
        eye_resolution: state
            .is_running()
            .then(|| state.eye_resolution(&cfg))
            .flatten(),
        busy: false,
    }
}

/// Tear the runtime down and clear all state. Idempotent. Registered with [`crate::lifecycle`], so it
/// runs on uninject — where, if [`VrConfig::persist_instance`] is set, the instance and session are
/// kept alive (their handles stashed in the game process environment, the wrappers leaked) for a
/// reinject to reuse, sidestepping the runtime's per-process instance/session budget. Otherwise
/// everything is destroyed.
pub fn uninstall() {
    let persist = Config::lock_query(|c| c.vr.persist_instance);
    let mut state = VR_STATE.lock();
    if state.instance.is_some() {
        tracing::info!(target: "vr", persist, "uninstalling the OpenXR runtime");
    }
    state.teardown(persist);
}

/// The live VR runtime state, on the game's main thread. Locked briefly by [`update`] and held for a
/// frame by [`crate::vr::FrameContext`]. A const-constructible [`Mutex`] singleton, the same pattern
/// [`crate::capture`] uses.
pub(super) static VR_STATE: Mutex<VrState> = Mutex::new(VrState::new());

/// The maximum number of ~2 ms event polls to wait for a session to reach EXITING during the
/// teardown exit handshake. A hard cap so teardown cannot hang if the runtime never advances the
/// state; ~1 s is ample for a local compositor.
const TEARDOWN_EVENT_POLLS: u32 = 500;

/// The runtime state singleton. `instance == None` means the runtime is torn down (flatscreen); a
/// present `instance` with `session == None` should not occur (the session is created together with
/// the instance during bring-up), but the state models them separately for ordered teardown.
pub(super) struct VrState {
    pub(super) instance: Option<xr::Instance>,
    pub(super) system: Option<xr::SystemId>,
    pub(super) blend_mode: xr::EnvironmentBlendMode,
    pub(super) session: Option<Session>,
    /// The last bring-up attempt, for the retry cadence.
    last_attempt: Option<Instant>,
    /// The recenter baseline (cockpit frame). `None` until first recenter.
    pub(super) baseline: Option<Baseline>,
    /// The latest located head pose in LOCAL space, for [`crate::vr::recenter`]. `None` until a frame
    /// locates.
    pub(super) latest_head_pose: Option<xr::Posef>,
    /// The runtime's recommended per-eye render resolution (raw, before [`VrConfig::resolution_scale`]),
    /// cached at bring-up. Feeds [`crate::vr::native_eye_resolution`] so the engine can render each eye
    /// at the same size the swapchain uses. `None` while torn down.
    recommended_view: Option<(u32, u32)>,
    /// The runtime name reported at bring-up, cached for the debug UI. `None` while torn down.
    runtime_name: Option<String>,
}

impl VrState {
    const fn new() -> Self {
        Self {
            instance: None,
            system: None,
            blend_mode: xr::EnvironmentBlendMode::OPAQUE,
            session: None,
            last_attempt: None,
            baseline: None,
            latest_head_pose: None,
            recommended_view: None,
            runtime_name: None,
        }
    }

    pub(super) fn is_running(&self) -> bool {
        self.session.as_ref().is_some_and(|s| s.running)
    }

    /// The per-eye render resolution to drive the engine to: the runtime's recommended view size
    /// scaled by [`VrConfig::resolution_scale`], the same computation the swapchain uses
    /// ([`scaled_eye_size`]). `None` until bring-up cached the recommended size.
    pub(super) fn eye_resolution(&self, cfg: &VrConfig) -> Option<(u32, u32)> {
        self.recommended_view
            .map(|(w, h)| scaled_eye_size(w, h, cfg.resolution_scale))
    }

    /// Attempt the full bring-up (loader → instance → system → session → reference spaces) if the
    /// retry cadence allows. Any failure logs and leaves the state torn down for the next retry,
    /// except an unloadable loader ([`LoaderUnavailable`]), which is fatal.
    fn try_bring_up(&mut self, cfg: &VrConfig) {
        let now = Instant::now();
        if let Some(last) = self.last_attempt
            && now.duration_since(last).as_secs() < cfg.retry_interval_secs
        {
            return;
        }
        self.last_attempt = Some(now);

        if let Err(e) = self.bring_up(cfg) {
            // A missing loader is a deployment fault, not a transient one: retrying cannot conjure
            // the DLL, and the warn-and-continue path below would leave the game running flat for a
            // whole session while looking, in the log, exactly like a headset that is merely idle.
            // Fail loudly at the first attempt instead -- which is during startup, since bring-up is
            // tried on the first frames.
            //
            // Aborting rather than panicking: this runs on the game thread inside a detour, so an
            // unwind would cross back into the engine's C++ frames. The error above has already
            // reached the log and stdout, which is the part that has to survive.
            if e.downcast_ref::<LoaderUnavailable>().is_some() {
                tracing::error!(target: "vr", "fatal: {e:#}");
                std::process::abort();
            }
            tracing::warn!(
                target: "vr",
                "OpenXR bring-up failed (staying in flatscreen stereo, retrying in {}s): {e:#}",
                cfg.retry_interval_secs,
            );
            // Keep any successfully-reused/created handles for the next retry when persistence is on;
            // a stale stashed handle was already cleared by the acquire path.
            self.teardown(cfg.persist_instance);
        }
    }

    /// The bring-up steps, each surfacing a context-prefixed error. On success the instance, system,
    /// blend mode, and session (not yet running -- the event pump begins it on READY) are stored.
    fn bring_up(&mut self, cfg: &VrConfig) -> anyhow::Result<()> {
        let entry = load_entry(cfg).context("vr: loading the OpenXR loader")?;

        let available = entry
            .enumerate_extensions()
            .context("vr: enumerating OpenXR extensions")?;
        if !available.khr_d3d11_enable {
            anyhow::bail!("vr: the OpenXR runtime lacks XR_KHR_D3D11_enable");
        }

        let mut extensions = xr::ExtensionSet::default();
        extensions.khr_d3d11_enable = true;
        let instance = acquire_instance(&entry, &extensions, cfg.persist_instance)?;

        let system = instance
            .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
            .context("vr: acquiring the HMD system")?;

        let blend_mode = *instance
            .enumerate_environment_blend_modes(system, VIEW_TYPE)
            .context("vr: enumerating environment blend modes")?
            .first()
            .context("vr: the runtime reported no environment blend modes")?;

        // Cache the recommended per-eye view size for the native-resolution driver, so it can size
        // the engine's scene render targets to match the swapchain without re-enumerating each frame.
        let recommended_view = instance
            .enumerate_view_configuration_views(system, VIEW_TYPE)
            .ok()
            .and_then(|views| {
                views.first().map(|v| {
                    (
                        v.recommended_image_rect_width,
                        v.recommended_image_rect_height,
                    )
                })
            });

        let session = acquire_session(&instance, system, cfg)?;

        let runtime_name = instance.properties().ok().map(|props| {
            tracing::info!(
                target: "vr",
                runtime = %props.runtime_name,
                version = %props.runtime_version,
                "OpenXR runtime brought up",
            );
            props.runtime_name.to_string()
        });

        self.instance = Some(instance);
        self.system = Some(system);
        self.blend_mode = blend_mode;
        self.session = Some(session);
        self.recommended_view = recommended_view;
        self.runtime_name = runtime_name;
        Ok(())
    }

    /// Pump OpenXR events: session-state transitions (READY → begin, STOPPING → end), instance loss,
    /// and lost events. On a transition to a lost/exiting state, or instance loss, tear the runtime
    /// down so the next [`update`] retries a clean bring-up.
    fn pump_events(&mut self) {
        // Take the instance out to satisfy the borrow checker (poll_event borrows the instance while
        // the handlers mutate `self`); restore it unless a handler cleared the session.
        let Some(instance) = self.instance.take() else {
            return;
        };
        // `xr::EventDataBuffer` is not `Send`, so it cannot live in the singleton; a fresh one per
        // pump is cheap (a fixed-size scratch buffer) and keeps `VrState: Send`.
        let mut events = xr::EventDataBuffer::new();
        let mut lost = false;
        loop {
            // Reduce each event to an owned action before touching `self`, so the `&mut events`
            // borrow held by the returned `Event` ends before the state handlers run.
            let action = match instance.poll_event(&mut events) {
                Ok(Some(xr::Event::SessionStateChanged(e))) => PumpAction::StateChanged(e.state()),
                Ok(Some(xr::Event::InstanceLossPending(_))) => PumpAction::InstanceLost,
                Ok(Some(xr::Event::EventsLost(e))) => {
                    tracing::warn!(target: "vr", "lost {} OpenXR events", e.lost_event_count());
                    continue;
                }
                Ok(Some(_)) => continue,
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!(target: "vr", "poll_event failed: {e}");
                    break;
                }
            };
            match action {
                PumpAction::StateChanged(new_state) => {
                    if self.on_session_state(new_state) {
                        lost = true;
                        break;
                    }
                }
                PumpAction::InstanceLost => {
                    tracing::warn!(target: "vr", "OpenXR instance loss pending; tearing down");
                    lost = true;
                    break;
                }
            }
        }
        if lost {
            // Restore the instance so `teardown` can destroy handles in order. A lost or exiting
            // session cannot be reused, so destroy everything and clear the stashes (persist=false)
            // rather than stashing a dead session for a reinject.
            self.instance = Some(instance);
            self.teardown(false);
        } else {
            self.instance = Some(instance);
        }
    }

    /// Handle a session-state transition. Returns `true` if the runtime should be torn down
    /// (EXITING / LOSS_PENDING).
    fn on_session_state(&mut self, state: xr::SessionState) -> bool {
        tracing::info!(target: "vr", "session state -> {state:?}");
        let Some(session) = self.session.as_mut() else {
            return false;
        };
        match state {
            xr::SessionState::READY => {
                if let Err(e) = session.handle.begin(VIEW_TYPE) {
                    tracing::error!(target: "vr", "session begin failed: {e}");
                    return true;
                }
                session.running = true;
            }
            xr::SessionState::STOPPING => {
                if let Err(e) = session.handle.end() {
                    tracing::error!(target: "vr", "session end failed: {e}");
                }
                session.running = false;
            }
            xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                return true;
            }
            _ => {}
        }
        false
    }

    /// Tear the runtime down in order: swapchain → session → instance. Ending a running session
    /// first is best-effort (the runtime may already be stopping). Clears all derived state.
    /// Walk a running session through the OpenXR exit handshake before it is destroyed: request
    /// exit, then pump events to `end()` it on STOPPING and wait for EXITING, so the runtime
    /// releases the session and the instance slot it holds. Destroying a still-running session
    /// instead leaves a headset runtime (e.g. WiVRn) holding the instance, and a reinject then fails
    /// to create a new one with `XR_ERROR_LIMIT_REACHED`. Bounded by [`TEARDOWN_EVENT_POLLS`] so it
    /// can never hang if the runtime never advances the state.
    fn end_session_cleanly(&mut self) {
        match self.session.as_mut() {
            Some(session) if session.running => {
                if let Err(e) = session.handle.request_exit() {
                    tracing::debug!(target: "vr", "request_exit during teardown failed: {e}");
                    return;
                }
            }
            // Not running (never begun) or no session: nothing to hand back to the runtime.
            _ => return,
        }
        let Some(instance) = self.instance.take() else {
            return;
        };
        let mut events = xr::EventDataBuffer::new();
        for _ in 0..TEARDOWN_EVENT_POLLS {
            match instance.poll_event(&mut events) {
                Ok(Some(xr::Event::SessionStateChanged(e))) => match e.state() {
                    xr::SessionState::STOPPING => {
                        if let Some(session) = self.session.as_mut() {
                            if let Err(e) = session.handle.end() {
                                tracing::debug!(target: "vr", "session end during teardown failed: {e}");
                            }
                            session.running = false;
                        }
                    }
                    xr::SessionState::EXITING | xr::SessionState::IDLE => break,
                    _ => {}
                },
                Ok(Some(_)) => {}
                // No event yet: the runtime has not advanced the state; wait briefly and re-poll.
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(2)),
                Err(e) => {
                    tracing::debug!(target: "vr", "poll_event during teardown failed: {e}");
                    break;
                }
            }
        }
        self.instance = Some(instance);
    }

    /// Tear the runtime down. When `persist` is set, keep the instance and session alive for a
    /// reinject to reuse (stash their handles, leak the wrappers, and *do not* end the session — an
    /// ended session cannot be resumed), sidestepping the runtime's per-process instance/session
    /// budget. When not set, end and destroy everything and clear the stashes so a later bring-up
    /// starts fresh (used when VR is genuinely stopped: `vr.enabled` off, or a lost session).
    fn teardown(&mut self, persist: bool) {
        // Drain the GPU first, and release the flip block, so the game's own present path resumes
        // against an idle pipeline rather than deadlocking in a timestamp-query readback (see
        // `blit::drain_gpu`); the swapchain is destroyed either way (persisting keeps only the
        // session handle). When destroying, walk the session through the OpenXR exit handshake first.
        if self.session.is_some() {
            blit::drain_gpu();
            if !persist {
                self.end_session_cleanly();
            }
        }
        graphics_engine::BLOCK_FLIP.store(false, std::sync::atomic::Ordering::Relaxed);

        if persist {
            if let Some(session) = self.session.take() {
                persist_session(session);
            }
            if let Some(instance) = self.instance.take() {
                stash_instance(instance);
            }
        } else {
            if let Some(mut session) = self.session.take() {
                // The exit handshake ran above; destroy the swapchain (before the session handle),
                // then dropping `session` drops the frame stream/waiter and the session handle.
                session.swapchain = None;
            }
            self.instance = None;
            clear_persisted();
        }
        self.system = None;
        self.baseline = None;
        self.latest_head_pose = None;
        self.recommended_view = None;
        self.runtime_name = None;
    }
}

/// An owned reduction of a pumped OpenXR event, decoupled from the borrowed [`xr::Event`] so the
/// event pump can act on `self` after the event buffer borrow ends.
enum PumpAction {
    /// A session-state transition to act on (READY → begin, STOPPING → end, EXITING → teardown).
    StateChanged(xr::SessionState),
    /// The instance is being lost; tear down.
    InstanceLost,
}
