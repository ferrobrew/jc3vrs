//! The OpenXR runtime: session lifecycle, event pump, and the per-frame API surface the Draw wiring
//! drives. This module owns the OpenXR instance, session, reference spaces, and stereo swapchain. The
//! per-frame loop ([`update`] → [`frame_begin`] → [`FrameContext`] → per-eye blit → [`present_and_submit`])
//! is driven from `hooks::game::game_update_render`; the per-eye render parameters flow to the camera
//! hook through [`frame`]'s separate slot ([`render_params`]), not the frame-held runtime lock. See
//! `docs/mod/vr-runtime.md` for the loop end to end.
//!
//! ## Loader route
//!
//! The OpenXR loader is **dynamically loaded at runtime** (`xr::Entry::load_from`), not linked. The
//! `static` loader route (build the Khronos loader through cmake against the xwin/clang-cl cross
//! toolchain) does not build in this environment -- cmake selects the Ninja generator, which the
//! cross toolchain lacks -- so the portable choice is the runtime loader. The loader DLL defaults to
//! `openxr_loader.dll` next to the payload DLL ([`crate::module::get_path`]) and is overridable via
//! [`crate::config::VrConfig::loader_path`]. When the loader is absent the mod stays in flatscreen
//! stereo and retries on the configured cadence.
//!
//! ## Threading
//!
//! Everything runs on the game's main thread, the same model as [`crate::capture`]: a single
//! [`Mutex<VrState>`] singleton, locked briefly on that thread. The game's `ID3D11Device` is fetched
//! from the graphics engine singleton at session-create time under the same null-guarding
//! [`crate::capture`] uses; the device is never stored (so the state carries no raw device pointer
//! across threads). All OpenXR handles are `Send` (`Arc`-backed handles), so the state is a safe
//! singleton.
//!
//! ## Degradation and retry
//!
//! Bring-up failure at any stage logs on target `"vr"` and leaves the mod in flatscreen stereo;
//! [`update`] retries the whole bring-up every [`crate::config::VrConfig::retry_interval_secs`] while
//! `vr.enabled`. Turning `vr.enabled` off, or [`crate::lifecycle`] shutdown, tears the runtime down
//! in order (swapchain → session → instance) so the OpenXR instance never outlives the DLL.

pub mod projection;

use std::time::Instant;

use anyhow::Context as _;
use openxr as xr;
use openxr::sys::Handle as _;
use parking_lot::{Mutex, MutexGuard};
use windows::core::Interface as _;

use crate::config::Config;

pub use config::{BlitGamma, ProjectionConvention, VrConfig};
pub use frame::{
    EyeRenderParams, begin_render_frame, clear_render_params, cull_projection_standard,
    render_params,
};
pub use projection::{Fov, OffAxisProjection};

mod blit;
mod config;
mod frame;
mod mirror;
mod resolution;

pub use blit::present_and_submit;
pub use mirror::present_mirror;
pub use resolution::apply_native_resolution;

/// The OpenXR view configuration: standard stereo, two views (one per eye).
const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;
/// The number of views (eyes), and the swapchain array size (one slice per eye).
const VIEW_COUNT: u32 = 2;
/// The maximum number of ~2 ms event polls to wait for a session to reach EXITING during the
/// teardown exit handshake. A hard cap so teardown cannot hang if the runtime never advances the
/// state; ~1 s is ample for a local compositor.
const TEARDOWN_EVENT_POLLS: u32 = 500;

/// The live VR runtime state, on the game's main thread. Locked briefly by [`update`] and held for a
/// frame by [`FrameContext`]. A const-constructible [`Mutex`] singleton, the same pattern
/// [`crate::capture`] uses.
static VR_STATE: Mutex<VrState> = Mutex::new(VrState::new());

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
        mirror::teardown();
        uninstall();
    });
}

/// The once-per-frame entry point, called from the game thread by `hooks::game::game_update_render`.
/// Pumps OpenXR events, drives bring-up/retry/teardown per config, and
/// returns whether a session is currently running (so the caller can decide whether to submit VR
/// frames). Never panics on OpenXR failure -- failures degrade to flatscreen stereo and are retried.
pub fn update() -> bool {
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
}

/// Snapshot the VR runtime state for the debug UI. Locks the config and the runtime state briefly.
pub fn status() -> VrStatus {
    let cfg = Config::lock_query(|c| c.vr.clone());
    let state = VR_STATE.lock();
    VrStatus {
        enabled: cfg.enabled,
        instance_up: state.instance.is_some(),
        running: state.is_running(),
        runtime_name: state.runtime_name.clone(),
        eye_resolution: state
            .is_running()
            .then(|| state.eye_resolution(&cfg))
            .flatten(),
    }
}

/// The per-eye render resolution the engine should target while a session is running: the runtime's
/// recommended view size × [`VrConfig::resolution_scale`], matching the stereo swapchain so the blit
/// is a straight scale-1 pass. `None` when no session is up or the recommended size is unknown. Read
/// by [`resolution`] once per frame.
pub fn native_eye_resolution() -> Option<(u32, u32)> {
    let cfg = Config::lock_query(|c| c.vr.clone());
    let state = VR_STATE.lock();
    if !state.is_running() {
        return None;
    }
    state.eye_resolution(&cfg)
}

/// Scale a raw recommended per-eye view size by `resolution_scale`, clamped to a small positive
/// minimum (and at least 1 px each axis). Shared by the swapchain and the native-resolution driver so
/// the engine renders each eye at exactly the swapchain size.
fn scaled_eye_size(width: u32, height: u32, resolution_scale: f32) -> (u32, u32) {
    let scale = resolution_scale.max(0.1);
    let w = ((width as f32) * scale).round() as u32;
    let h = ((height as f32) * scale).round() as u32;
    (w.max(1), h.max(1))
}

/// Recenter the cockpit: re-base the stored baseline from the latest located VIEW-space pose, taking
/// its position and yaw only (the cockpit model). The frame loop consumes the baseline when
/// mapping per-eye poses (see [`frame`]). No-op until a frame has located a head pose.
pub fn recenter() {
    let mut state = VR_STATE.lock();
    match state.latest_head_pose {
        Some(pose) => {
            state.baseline = Some(Baseline::from_pose(pose));
            tracing::info!(target: "vr", "recentered the cockpit baseline");
        }
        None => {
            tracing::warn!(target: "vr", "recenter requested before any head pose was located");
        }
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

/// Begin an OpenXR frame: `wait_frame` + `begin_frame` + `locate_views`, returning a [`FrameContext`]
/// that holds the runtime lock for the duration of the frame. Returns `None` when no session is
/// running or the frame could not begin (the caller then renders flatscreen). The returned context
/// carries the per-eye poses (relative to the recenter baseline), FOVs, off-axis projections, and the
/// predicted display time; call [`FrameContext::should_render`] to decide whether to render or submit
/// an empty frame. Called from `hooks::game::game_update_render`.
pub fn frame_begin() -> Option<FrameContext> {
    let mut guard = VR_STATE.lock();

    if !guard.is_running() {
        return None;
    }

    let cfg = Config::lock_query(|c| c.vr.clone());
    match guard.begin_frame(&cfg) {
        Ok(frame) => Some(FrameContext {
            guard,
            frame,
            image_acquired: false,
        }),
        Err(e) => {
            tracing::warn!(target: "vr", "frame begin failed: {e:#}");
            None
        }
    }
}

/// A per-eye view for the frame in flight: pose relative to the recenter baseline, the raw HMD FOV,
/// and the off-axis projection built from it (both depth conventions, see [`projection`]).
#[derive(Copy, Clone)]
pub struct EyeView {
    /// The eye pose (position + orientation) relative to the recenter baseline, in the cockpit
    /// frame. When no baseline is set this is the raw LOCAL-space pose. This drives the *game camera*
    /// (so recentering re-orients the game world); it is NOT the compositor submission pose.
    pub pose: xr::Posef,
    /// The raw located eye pose in LOCAL space (before rebasing), i.e. where the eye actually is. This
    /// is the pose submitted to the compositor's projection layer: the layer is composited in LOCAL
    /// space, so its pose must describe the eye's true position, or the compositor reprojects the image
    /// to a plane offset by the recenter baseline. Equal to [`pose`](Self::pose) until the first
    /// recenter.
    pub raw_pose: xr::Posef,
    /// The eye's field of view, as reported by `locate_views`.
    pub fov: xr::Fovf,
    /// The off-axis projection for [`fov`](Self::fov). Write [`standard_depth`]
    /// (`OffAxisProjection::standard_depth`) into `m_Projection` before `SetupRenderCamera`
    /// (`docs/engine/rendering.md` §2.7 / blocker 1).
    pub projection: OffAxisProjection,
}

/// A swapchain image reference for one eye, handed to the per-eye blit. The swapchain is a single
/// 2-slice texture array; both eyes share the same acquired texture and are distinguished by
/// [`array_index`](Self::array_index). The texture is runtime-owned -- wrap it borrowed (no `AddRef`)
/// and do not release it.
#[derive(Copy, Clone)]
pub struct EyeImage {
    /// The acquired swapchain texture (`ID3D11Texture2D`), as a raw COM pointer. Wrap with
    /// `ID3D11Texture2D::from_raw` borrowed for the blit; the runtime owns it.
    pub texture: *mut std::ffi::c_void,
    /// The array slice for this eye (`0` = left, `1` = right).
    pub array_index: u32,
    /// The swapchain's DXGI format, so the per-eye blit can build a matching view / conversion.
    pub format: u32,
}

/// The frame in flight. Holds the runtime lock, so it must be dropped (or consumed via [`frame_end`])
/// before [`update`] or another [`frame_begin`] is called on the same thread. Carries the per-eye
/// views and the predicted display time; exposes the swapchain acquire/release the per-eye blit needs.
pub struct FrameContext {
    guard: MutexGuard<'static, VrState>,
    frame: FrameData,
    image_acquired: bool,
}

impl FrameContext {
    /// Whether the runtime wants the scene rendered this frame. When `false` the caller should skip
    /// rendering and call [`frame_end`] to submit an empty frame (the runtime is idle/occluded).
    pub fn should_render(&self) -> bool {
        self.frame.should_render
    }

    /// The predicted display time for this frame, for pose-dependent work and the frame submit.
    pub fn predicted_display_time(&self) -> xr::Time {
        self.frame.predicted_display_time
    }

    /// The per-eye view (pose relative to the recenter baseline, FOV, off-axis projection). `eye` is
    /// `0` (left) or `1` (right).
    pub fn eye_view(&self, eye: usize) -> EyeView {
        self.frame.eyes[eye]
    }

    /// Acquire and wait on the stereo swapchain image (created lazily on first use). Call once per
    /// frame before rendering; the two eyes are array slices of the returned image
    /// ([`eye_image`](Self::eye_image)). No-op if already acquired this frame.
    pub fn acquire(&mut self) -> anyhow::Result<()> {
        if self.image_acquired {
            return Ok(());
        }
        let cfg = Config::lock_query(|c| c.vr.clone());
        self.guard.acquire_swapchain_image(&cfg)?;
        self.image_acquired = true;
        self.frame.image_ever_acquired = true;
        Ok(())
    }

    /// The swapchain image for `eye` (`0` = left, `1` = right), valid only between [`acquire`] and
    /// [`release`]. `None` until [`acquire`] has run. The per-eye blit copies the game's captured eye
    /// texture into this image's `array_index` slice.
    ///
    /// [`acquire`]: Self::acquire
    /// [`release`]: Self::release
    pub fn eye_image(&self, eye: usize) -> Option<EyeImage> {
        if !self.image_acquired {
            return None;
        }
        let sc = self.guard.session.as_ref()?.swapchain.as_ref()?;
        Some(EyeImage {
            texture: sc.acquired_texture()?,
            array_index: eye as u32,
            format: sc.format,
        })
    }

    /// Release the swapchain image after the blit. No-op if not acquired.
    pub fn release(&mut self) -> anyhow::Result<()> {
        if !self.image_acquired {
            return Ok(());
        }
        self.guard.release_swapchain_image()?;
        self.image_acquired = false;
        Ok(())
    }

    /// End the frame: submit the world projection layer (or an empty frame when
    /// [`should_render`](Self::should_render) is false or the swapchain was never acquired) and
    /// consume the context, releasing the runtime lock. HUD quad layers become additional layers here
    /// in a later wave (`docs/mod/hud.md`); the surface takes only the world layer today.
    pub fn frame_end(mut self) -> anyhow::Result<()> {
        // Release any still-held image before submitting, so a caller that forgot to release does
        // not deadlock the swapchain.
        if self.image_acquired {
            self.release()?;
        }
        let submit_world = self.frame.should_render && self.frame.image_ever_acquired;
        self.guard.end_frame(&self.frame, submit_world)
    }
}

/// The per-frame data captured at [`frame_begin`], carried by the [`FrameContext`].
struct FrameData {
    predicted_display_time: xr::Time,
    should_render: bool,
    eyes: [EyeView; 2],
    /// Whether the swapchain image was acquired at some point this frame (so `frame_end` knows
    /// whether a world layer can be submitted).
    image_ever_acquired: bool,
}

/// An owned reduction of a pumped OpenXR event, decoupled from the borrowed [`xr::Event`] so the
/// event pump can act on `self` after the event buffer borrow ends.
enum PumpAction {
    /// A session-state transition to act on (READY → begin, STOPPING → end, EXITING → teardown).
    StateChanged(xr::SessionState),
    /// The instance is being lost; tear down.
    InstanceLost,
}

/// The recenter baseline: a position and a yaw-only orientation, re-based from the latest VIEW-space
/// pose. Per-eye poses are expressed relative to this transform (the cockpit model).
#[derive(Copy, Clone)]
struct Baseline {
    /// The world-from-baseline transform (position + yaw). Per-eye poses are re-based by its inverse.
    position: glam::Vec3,
    /// Yaw-only orientation (rotation about the up axis).
    yaw: glam::Quat,
}

impl Baseline {
    /// Extract the position and yaw-only orientation from a located VIEW-space pose.
    fn from_pose(pose: xr::Posef) -> Self {
        let orientation = glam::Quat::from_xyzw(
            pose.orientation.x,
            pose.orientation.y,
            pose.orientation.z,
            pose.orientation.w,
        );
        // Yaw only: project the orientation onto rotation about the Y (up) axis. Zero the X/Z
        // components of the quaternion and renormalize; a degenerate (looking straight up/down)
        // quaternion falls back to identity yaw.
        let yaw = glam::Quat::from_xyzw(0.0, orientation.y, 0.0, orientation.w);
        let yaw = if yaw.length_squared() > 1e-6 {
            yaw.normalize()
        } else {
            glam::Quat::IDENTITY
        };
        Self {
            position: glam::Vec3::new(pose.position.x, pose.position.y, pose.position.z),
            yaw,
        }
    }

    /// Re-base a located pose into the baseline (cockpit) frame: `baseline⁻¹ · pose`.
    fn rebase(&self, pose: xr::Posef) -> xr::Posef {
        let pos = glam::Vec3::new(pose.position.x, pose.position.y, pose.position.z);
        let rot = glam::Quat::from_xyzw(
            pose.orientation.x,
            pose.orientation.y,
            pose.orientation.z,
            pose.orientation.w,
        );
        let inv_yaw = self.yaw.conjugate();
        let rel_pos = inv_yaw * (pos - self.position);
        let rel_rot = inv_yaw * rot;
        xr::Posef {
            orientation: xr::Quaternionf {
                x: rel_rot.x,
                y: rel_rot.y,
                z: rel_rot.z,
                w: rel_rot.w,
            },
            position: xr::Vector3f {
                x: rel_pos.x,
                y: rel_pos.y,
                z: rel_pos.z,
            },
        }
    }
}

/// The runtime state singleton. `instance == None` means the runtime is torn down (flatscreen); a
/// present `instance` with `session == None` should not occur (the session is created together with
/// the instance during bring-up), but the state models them separately for ordered teardown.
struct VrState {
    instance: Option<xr::Instance>,
    system: Option<xr::SystemId>,
    blend_mode: xr::EnvironmentBlendMode,
    session: Option<Session>,
    /// The last bring-up attempt, for the retry cadence.
    last_attempt: Option<Instant>,
    /// The recenter baseline (cockpit frame). `None` until first recenter.
    baseline: Option<Baseline>,
    /// The latest located head pose in LOCAL space, for [`recenter`]. `None` until a frame locates.
    latest_head_pose: Option<xr::Posef>,
    /// The runtime's recommended per-eye render resolution (raw, before [`VrConfig::resolution_scale`]),
    /// cached at bring-up. Feeds [`native_eye_resolution`] so the engine can render each eye at the
    /// same size the swapchain uses. `None` while torn down.
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

    fn is_running(&self) -> bool {
        self.session.as_ref().is_some_and(|s| s.running)
    }

    /// The per-eye render resolution to drive the engine to: the runtime's recommended view size
    /// scaled by [`VrConfig::resolution_scale`], the same computation the swapchain uses
    /// ([`scaled_eye_size`]). `None` until bring-up cached the recommended size.
    fn eye_resolution(&self, cfg: &VrConfig) -> Option<(u32, u32)> {
        self.recommended_view
            .map(|(w, h)| scaled_eye_size(w, h, cfg.resolution_scale))
    }

    /// Attempt the full bring-up (loader → instance → system → session → reference spaces) if the
    /// retry cadence allows. Any failure logs and leaves the state torn down for the next retry.
    fn try_bring_up(&mut self, cfg: &VrConfig) {
        let now = Instant::now();
        if let Some(last) = self.last_attempt
            && now.duration_since(last).as_secs() < cfg.retry_interval_secs
        {
            return;
        }
        self.last_attempt = Some(now);

        if let Err(e) = self.bring_up(cfg) {
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

    /// Begin a frame and locate the per-eye views, re-based into the cockpit frame. Updates the
    /// latest head pose (for [`recenter`]) from the mid-eye pose.
    fn begin_frame(&mut self, cfg: &VrConfig) -> anyhow::Result<FrameData> {
        let session = self
            .session
            .as_mut()
            .context("vr: no session for frame begin")?;

        let frame_state = session.frame_wait.wait().context("vr: wait_frame failed")?;
        session
            .frame_stream
            .begin()
            .context("vr: begin_frame failed")?;

        let mut eyes = [EyeView {
            pose: xr::Posef::IDENTITY,
            raw_pose: xr::Posef::IDENTITY,
            fov: xr::Fovf {
                angle_left: 0.0,
                angle_right: 0.0,
                angle_up: 0.0,
                angle_down: 0.0,
            },
            projection: OffAxisProjection::new(
                Fov {
                    left: 0.0,
                    right: 0.0,
                    up: 0.0,
                    down: 0.0,
                },
                cfg.near_clip,
                cfg.far_clip,
            ),
        }; 2];

        let mut head_pose = None;
        if frame_state.should_render {
            let (_flags, views) = session
                .handle
                .locate_views(
                    VIEW_TYPE,
                    frame_state.predicted_display_time,
                    &session.local,
                )
                .context("vr: locate_views failed")?;
            if views.len() >= 2 {
                head_pose = Some(mid_pose(views[0].pose, views[1].pose));
                for (eye, view) in eyes.iter_mut().zip(views.iter()) {
                    let pose = match self.baseline {
                        Some(b) => b.rebase(view.pose),
                        None => view.pose,
                    };
                    *eye = EyeView {
                        pose,
                        raw_pose: view.pose,
                        fov: view.fov,
                        projection: OffAxisProjection::new(
                            fov_from_xr(view.fov),
                            cfg.near_clip,
                            cfg.far_clip,
                        ),
                    };
                }
            }
        }

        if let Some(p) = head_pose {
            self.latest_head_pose = Some(p);
        }

        Ok(FrameData {
            predicted_display_time: frame_state.predicted_display_time,
            should_render: frame_state.should_render,
            eyes,
            image_ever_acquired: false,
        })
    }

    /// Acquire and wait on the swapchain image, creating the swapchain lazily on first use.
    fn acquire_swapchain_image(&mut self, cfg: &VrConfig) -> anyhow::Result<()> {
        let instance = self
            .instance
            .as_ref()
            .context("vr: no instance for swapchain acquire")?
            .clone();
        let system = self.system.context("vr: no system for swapchain acquire")?;
        let session = self
            .session
            .as_mut()
            .context("vr: no session for swapchain acquire")?;
        if session.swapchain.is_none() {
            session.swapchain = Some(Swapchain::create(&instance, system, &session.handle, cfg)?);
        }
        let sc = session
            .swapchain
            .as_mut()
            .expect("swapchain was just ensured");
        sc.acquire()
    }

    /// Release the swapchain image.
    fn release_swapchain_image(&mut self) -> anyhow::Result<()> {
        let session = self
            .session
            .as_mut()
            .context("vr: no session for swapchain release")?;
        let sc = session
            .swapchain
            .as_mut()
            .context("vr: no swapchain to release")?;
        sc.release()
    }

    /// End the frame, submitting the world projection layer when `submit_world`, else an empty
    /// frame. Borrows the session's fields disjointly so the layer can reference the swapchain and
    /// local space while `frame_stream` is borrowed mutably.
    fn end_frame(&mut self, frame: &FrameData, submit_world: bool) -> anyhow::Result<()> {
        let blend_mode = self.blend_mode;
        let session = self
            .session
            .as_mut()
            .context("vr: no session for frame end")?;

        if !submit_world {
            return session
                .frame_stream
                .end(frame.predicted_display_time, blend_mode, &[])
                .context("vr: end_frame (empty) failed");
        }

        let sc = session
            .swapchain
            .as_ref()
            .context("vr: no swapchain for world layer")?;
        let extent = xr::Extent2Di {
            width: sc.width as i32,
            height: sc.height as i32,
        };
        let rect = xr::Rect2Di {
            offset: xr::Offset2Di { x: 0, y: 0 },
            extent,
        };
        // Submit the RAW located eye poses (where the eyes actually are in LOCAL space), not the
        // rebased poses that drive the game camera. The projection layer is composited in LOCAL space,
        // so the compositor treats the submitted pose as the image's viewpoint and reprojects to the
        // real eye; feeding it the rebased pose displaces the image by the recenter baseline (the
        // floating, angled plane after F7). The recenter is already baked into the rendered content
        // via the game camera, so the compositor must see the true eye pose here.
        let views = [
            xr::CompositionLayerProjectionView::new()
                .pose(frame.eyes[0].raw_pose)
                .fov(frame.eyes[0].fov)
                .sub_image(
                    xr::SwapchainSubImage::new()
                        .swapchain(&sc.handle)
                        .image_array_index(0)
                        .image_rect(rect),
                ),
            xr::CompositionLayerProjectionView::new()
                .pose(frame.eyes[1].raw_pose)
                .fov(frame.eyes[1].fov)
                .sub_image(
                    xr::SwapchainSubImage::new()
                        .swapchain(&sc.handle)
                        .image_array_index(1)
                        .image_rect(rect),
                ),
        ];
        let layer = xr::CompositionLayerProjection::new()
            .space(&session.local)
            .views(&views);
        session
            .frame_stream
            .end(frame.predicted_display_time, blend_mode, &[&layer])
            .context("vr: end_frame failed")
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
        crate::hooks::graphics_engine::graphics_engine::BLOCK_FLIP
            .store(false, std::sync::atomic::Ordering::Relaxed);

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

/// A created OpenXR session and its per-session resources. `running` tracks the READY..STOPPING
/// window driven by the event pump.
struct Session {
    handle: xr::Session<xr::D3D11>,
    frame_wait: xr::FrameWaiter,
    frame_stream: xr::FrameStream<xr::D3D11>,
    /// The LOCAL reference space -- the cockpit-relative world frame.
    local: xr::Space,
    /// The stereo swapchain, created lazily once the session is running and first rendered.
    swapchain: Option<Swapchain>,
    running: bool,
}

impl Session {
    /// Create the session against the game's `ID3D11Device`, after checking the D3D11 graphics
    /// requirements the spec requires. The device is fetched from the graphics engine singleton
    /// under [`crate::capture`]'s null-guarding and is not stored.
    fn create(
        instance: &xr::Instance,
        system: xr::SystemId,
        _cfg: &VrConfig,
    ) -> anyhow::Result<Self> {
        // The spec requires querying graphics requirements before create_session; the returned
        // min feature level is informational for us (we share the engine's already-created device).
        let requirements = instance
            .graphics_requirements::<xr::D3D11>(system)
            .context("vr: querying D3D11 graphics requirements")?;
        tracing::info!(
            target: "vr",
            min_feature_level = requirements.min_feature_level,
            "D3D11 graphics requirements",
        );

        let device_ptr = with_engine_device(|device| device.m_Device.as_raw())?;

        let (handle, frame_wait, frame_stream) = unsafe {
            instance
                .create_session::<xr::D3D11>(
                    system,
                    &xr::d3d::SessionCreateInfoD3D11 {
                        device: device_ptr.cast(),
                    },
                )
                .context("vr: create_session failed")?
        };

        let local = handle
            .create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
            .context("vr: creating the LOCAL reference space")?;

        Ok(Self {
            handle,
            frame_wait,
            frame_stream,
            local,
            swapchain: None,
            running: false,
        })
    }
}

/// The stereo swapchain: a single 2-slice texture array (one slice per eye), sized from the
/// runtime's recommended per-eye resolution scaled by `vr.resolution_scale`, in a negotiated format.
struct Swapchain {
    handle: xr::Swapchain<xr::D3D11>,
    width: u32,
    height: u32,
    /// The DXGI format actually chosen (recorded for the per-eye blit, which must match/convert).
    format: u32,
    /// The enumerated swapchain images (raw `ID3D11Texture2D` pointers as `usize`, so the state stays
    /// `Send`; runtime-owned). Cast back to a pointer at [`Swapchain::acquired_texture`].
    images: Vec<usize>,
    /// The index returned by the most recent `acquire_image`, valid until `release_image`.
    acquired_index: Option<u32>,
}

impl Swapchain {
    /// Create the swapchain from the recommended view-configuration resolution × `resolution_scale`,
    /// negotiating a format from `enumerate_swapchain_formats` (preferring sRGB 8-bit).
    fn create(
        instance: &xr::Instance,
        system: xr::SystemId,
        session: &xr::Session<xr::D3D11>,
        cfg: &VrConfig,
    ) -> anyhow::Result<Self> {
        let views = instance
            .enumerate_view_configuration_views(system, VIEW_TYPE)
            .context("vr: enumerating view configuration views")?;
        let view = views
            .first()
            .context("vr: the runtime reported no view configuration views")?;

        let (width, height) = scaled_eye_size(
            view.recommended_image_rect_width,
            view.recommended_image_rect_height,
            cfg.resolution_scale,
        );

        let formats = session
            .enumerate_swapchain_formats()
            .context("vr: enumerating swapchain formats")?;
        let format = negotiate_format(&formats)?;

        let handle = session
            .create_swapchain(&xr::SwapchainCreateInfo {
                create_flags: xr::SwapchainCreateFlags::EMPTY,
                usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT
                    | xr::SwapchainUsageFlags::SAMPLED
                    | xr::SwapchainUsageFlags::TRANSFER_DST,
                format,
                sample_count: 1,
                width,
                height,
                face_count: 1,
                array_size: VIEW_COUNT,
                mip_count: 1,
            })
            .context("vr: create_swapchain failed")?;

        let images: Vec<usize> = handle
            .enumerate_images()
            .context("vr: enumerating swapchain images")?
            .into_iter()
            .map(|ptr| ptr as usize)
            .collect();

        tracing::info!(
            target: "vr",
            width,
            height,
            format,
            image_count = images.len(),
            "created the stereo swapchain",
        );

        Ok(Self {
            handle,
            width,
            height,
            format,
            images,
            acquired_index: None,
        })
    }

    fn acquire(&mut self) -> anyhow::Result<()> {
        let index = self
            .handle
            .acquire_image()
            .context("vr: acquire_image failed")?;
        self.handle
            .wait_image(xr::Duration::INFINITE)
            .context("vr: wait_image failed")?;
        self.acquired_index = Some(index);
        Ok(())
    }

    fn release(&mut self) -> anyhow::Result<()> {
        self.handle
            .release_image()
            .context("vr: release_image failed")?;
        self.acquired_index = None;
        Ok(())
    }

    /// The currently acquired texture, or `None` when no image is acquired.
    fn acquired_texture(&self) -> Option<*mut std::ffi::c_void> {
        let index = self.acquired_index? as usize;
        self.images.get(index).map(|&p| p as *mut std::ffi::c_void)
    }
}

/// The Windows process environment variables that persist the OpenXR instance and session handles
/// across inject/uninject cycles (see [`VrConfig::persist_instance`]). The payload DLL unmaps on
/// uninject, so a payload static cannot survive; the game process's environment block does. The
/// runtime allows only a small number of instances *and* sessions per process, so a reinject must
/// reuse both rather than create new ones.
const INSTANCE_STASH_VAR: windows::core::PCWSTR = windows::core::w!("JC3VRS_XR_INSTANCE");
const SESSION_STASH_VAR: windows::core::PCWSTR = windows::core::w!("JC3VRS_XR_SESSION");

/// Store a handle value in the game process's environment under `var`, as hex.
fn stash_handle(var: windows::core::PCWSTR, raw: u64) {
    let value: Vec<u16> = format!("{raw:#x}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let _ = windows::Win32::System::Environment::SetEnvironmentVariableW(
            var,
            windows::core::PCWSTR(value.as_ptr()),
        );
    }
}

/// Read a persisted handle value from the game process's environment, if set and non-zero.
fn stashed_handle(var: windows::core::PCWSTR) -> Option<u64> {
    let mut buf = [0u16; 32];
    let len = unsafe {
        windows::Win32::System::Environment::GetEnvironmentVariableW(var, Some(&mut buf))
    };
    if len == 0 || len as usize >= buf.len() {
        return None;
    }
    let text = String::from_utf16_lossy(&buf[..len as usize]);
    u64::from_str_radix(text.trim().trim_start_matches("0x"), 16)
        .ok()
        .filter(|&v| v != 0)
}

/// Clear a persisted handle (a stale one that failed to reuse).
fn clear_handle(var: windows::core::PCWSTR) {
    unsafe {
        let _ = windows::Win32::System::Environment::SetEnvironmentVariableW(
            var,
            windows::core::PCWSTR::null(),
        );
    }
}

/// Clear both persisted handles. Called when the runtime is genuinely stopped (not persisted for a
/// reinject) — a session loss, or `vr.enabled` turned off — so a later bring-up starts fresh.
fn clear_persisted() {
    clear_handle(INSTANCE_STASH_VAR);
    clear_handle(SESSION_STASH_VAR);
}

/// Acquire an OpenXR instance: reuse a persisted handle if `persist` is set and one is stashed and
/// still live, otherwise create a fresh one. A stashed handle that fails to re-wrap or validate (the
/// runtime dropped it) is cleared and falls back to a fresh create.
fn acquire_instance(
    entry: &xr::Entry,
    extensions: &xr::ExtensionSet,
    persist: bool,
) -> anyhow::Result<xr::Instance> {
    if persist && let Some(raw) = stashed_handle(INSTANCE_STASH_VAR) {
        match unsafe { reuse_instance(entry, raw, extensions) } {
            Ok(instance) => {
                tracing::info!(target: "vr", handle = format_args!("{raw:#x}"), "reused the persisted OpenXR instance");
                return Ok(instance);
            }
            Err(e) => {
                tracing::warn!(target: "vr", "the persisted OpenXR instance is unusable ({e:#}); creating a fresh one");
                clear_handle(INSTANCE_STASH_VAR);
            }
        }
    }
    entry
        .create_instance(
            &xr::ApplicationInfo {
                application_name: "jc3vrs",
                application_version: 0,
                engine_name: "jc3vrs",
                engine_version: 0,
                api_version: xr::Version::new(1, 0, 0),
            },
            extensions,
            &[],
        )
        .context("vr: creating the OpenXR instance")
}

/// Re-wrap a persisted OpenXR instance handle: load the extension function table for it and confirm
/// it is live. See [`VrConfig::persist_instance`].
///
/// # Safety
/// `raw` must be an instance handle that was created with `extensions` and has not been destroyed.
unsafe fn reuse_instance(
    entry: &xr::Entry,
    raw: u64,
    extensions: &xr::ExtensionSet,
) -> anyhow::Result<xr::Instance> {
    let handle = xr::sys::Instance::from_raw(raw);
    let exts = unsafe { xr::InstanceExtensions::load(entry, handle, extensions) }
        .context("loading extensions for the persisted instance")?;
    let instance = unsafe { xr::Instance::from_raw(entry.clone(), handle, exts) }
        .context("wrapping the persisted instance handle")?;
    // Confirm the handle is actually live before committing to it.
    instance
        .properties()
        .context("querying the persisted instance")?;
    Ok(instance)
}

/// Persist an OpenXR instance across inject cycles: stash its handle in the game process's
/// environment and leak the wrapper so its `Drop` never calls `xrDestroyInstance`, keeping the handle
/// live for the process lifetime for a later reinject to reuse. Consumes the instance so the two
/// halves (stash the handle, suppress the destroy) cannot be split. See [`VrConfig::persist_instance`].
fn stash_instance(instance: xr::Instance) {
    stash_handle(INSTANCE_STASH_VAR, instance.as_raw().into_raw());
    std::mem::forget(instance);
}

/// Acquire an OpenXR session: reuse a persisted session if `cfg.persist_instance` is set and one is
/// stashed and still valid, otherwise create a fresh one. A stale stashed session is cleared and
/// falls back to a fresh create.
fn acquire_session(
    instance: &xr::Instance,
    system: xr::SystemId,
    cfg: &VrConfig,
) -> anyhow::Result<Session> {
    if cfg.persist_instance
        && let Some(raw) = stashed_handle(SESSION_STASH_VAR)
    {
        match unsafe { reuse_session(instance, raw) } {
            Ok(session) => {
                tracing::info!(target: "vr", handle = format_args!("{raw:#x}"), "reused the persisted OpenXR session");
                return Ok(session);
            }
            Err(e) => {
                tracing::warn!(target: "vr", "the persisted OpenXR session is unusable ({e:#}); creating a fresh one");
                clear_handle(SESSION_STASH_VAR);
            }
        }
    }
    Session::create(instance, system, cfg)
}

/// Re-wrap a persisted session handle: regenerate the frame waiter/stream (`Session::from_raw`) and
/// recreate the LOCAL reference space (which also validates the session still exists). The session
/// was persisted while `FOCUSED` (never ended), so `running` starts true; the swapchain is recreated
/// lazily on the first frame as usual.
///
/// # Safety
/// `raw` must be a D3D11 session handle created on `instance`, not currently inside a frame, and not
/// destroyed.
unsafe fn reuse_session(instance: &xr::Instance, raw: u64) -> anyhow::Result<Session> {
    let handle = xr::sys::Session::from_raw(raw);
    // An empty drop guard: the real one keeps the graphics device alive, but we share the game's
    // device, which outlives every VR session.
    let (session, frame_wait, frame_stream) =
        unsafe { xr::Session::<xr::D3D11>::from_raw(instance.clone(), handle, Box::new(())) };
    let local = session
        .create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
        .context("recreating the LOCAL reference space for the persisted session")?;
    Ok(Session {
        handle: session,
        frame_wait,
        frame_stream,
        local,
        swapchain: None,
        running: true,
    })
}

/// Persist a session across inject cycles: destroy its recreatable children (swapchain, reference
/// space) but keep the session handle alive — stash it and leak the wrapper — *without* ending it, so
/// a reinject can re-wrap and resume it (an ended session cannot be resumed). Consumes the session.
fn persist_session(session: Session) {
    let Session {
        handle,
        frame_wait,
        frame_stream,
        local,
        swapchain,
        running: _,
    } = session;
    // Destroy the cheap-to-recreate children; the reinject rebuilds them on the reused session.
    drop(swapchain);
    drop(local);
    // The frame waiter/stream hold session references but issue no XR destroy; dropping them just
    // releases those references (the leaked handle below keeps the session alive).
    drop(frame_wait);
    drop(frame_stream);
    stash_handle(SESSION_STASH_VAR, handle.as_raw().into_raw());
    std::mem::forget(handle);
}

/// Load the OpenXR loader (dynamic route). Uses [`VrConfig::loader_path`] if set, else
/// `openxr_loader.dll` next to the payload DLL, falling back to the platform default search
/// (`xr::Entry::load`) when no explicit path is configured and the payload-adjacent DLL is
/// missing or fails to load — a system-wide loader then still works. An explicit
/// `loader_path` does not fall back: the user asked for that loader specifically.
fn load_entry(cfg: &VrConfig) -> anyhow::Result<xr::Entry> {
    if let Some(path) = cfg.loader_path.clone().map(std::path::PathBuf::from) {
        tracing::info!(target: "vr", loader = %path.display(), "loading the configured OpenXR loader");
        return unsafe { xr::Entry::load_from(&path) }
            .with_context(|| format!("loader at {}", path.display()));
    }

    if let Some(path) =
        crate::module::get_path().and_then(|p| p.parent().map(|d| d.join("openxr_loader.dll")))
    {
        tracing::info!(target: "vr", loader = %path.display(), "loading the payload-adjacent OpenXR loader");
        match unsafe { xr::Entry::load_from(&path) } {
            Ok(entry) => return Ok(entry),
            Err(e) => {
                tracing::info!(
                    target: "vr",
                    "payload-adjacent loader unavailable ({e}); trying the default search path",
                );
            }
        }
    }

    tracing::info!(target: "vr", "loading the OpenXR loader from the default search path");
    unsafe { xr::Entry::load() }.context("loader on the default search path")
}

/// Negotiate a swapchain color format from the runtime's supported list, preferring an 8-bit sRGB
/// format (the eye captures resolve through the engine's LDR path). Falls back to the runtime's
/// first offered format, logging the choice. The game's captures may be a different format; the
/// per-eye blit bridges them.
fn negotiate_format(formats: &[u32]) -> anyhow::Result<u32> {
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
    };
    const PREFERRED: [u32; 2] = [
        DXGI_FORMAT_R8G8B8A8_UNORM_SRGB.0 as u32,
        DXGI_FORMAT_B8G8R8A8_UNORM_SRGB.0 as u32,
    ];

    if let Some(&format) = PREFERRED.iter().find(|f| formats.contains(f)) {
        tracing::info!(target: "vr", format, "negotiated a preferred sRGB swapchain format");
        return Ok(format);
    }
    let format = *formats
        .first()
        .context("vr: the runtime offered no swapchain formats")?;
    tracing::warn!(
        target: "vr",
        format,
        "no preferred sRGB format available; using the runtime's first offered format",
    );
    Ok(format)
}

/// Fetch the game's `ID3D11Device` from the graphics engine singleton, null-guarded exactly as
/// [`crate::capture`] does, and run `f` against it. The device is not retained past `f`.
fn with_engine_device<R>(
    f: impl FnOnce(&jc3gi::graphics_engine::device::Device) -> R,
) -> anyhow::Result<R> {
    let ge = unsafe { jc3gi::graphics_engine::graphics_engine::GraphicsEngine::get() }
        .context("vr: the graphics engine is unavailable")?;
    let device =
        unsafe { ge.m_Device.as_ref() }.context("vr: the graphics device is unavailable")?;
    Ok(f(device))
}

/// Convert an `xr::Fovf` (radian half-angles) into a [`Fov`] for the projection builder.
fn fov_from_xr(fov: xr::Fovf) -> Fov {
    Fov {
        left: fov.angle_left,
        right: fov.angle_right,
        up: fov.angle_up,
        down: fov.angle_down,
    }
}

/// The midpoint pose of the two eyes (position averaged, orientation from the left eye): a stand-in
/// head pose for the recenter baseline.
fn mid_pose(a: xr::Posef, b: xr::Posef) -> xr::Posef {
    xr::Posef {
        orientation: a.orientation,
        position: xr::Vector3f {
            x: 0.5 * (a.position.x + b.position.x),
            y: 0.5 * (a.position.y + b.position.y),
            z: 0.5 * (a.position.z + b.position.z),
        },
    }
}
