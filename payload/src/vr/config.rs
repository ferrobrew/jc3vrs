//! VR runtime configuration. See [`crate::vr`] and `docs/mod/vr-runtime.md`.

use serde::{Deserialize, Serialize};

/// Which depth convention the per-eye off-axis projection is written in, and where in the
/// `SetupRenderCamera` sequence it lands (`docs/engine/rendering.md` §2.7, `docs/mod/vr-runtime.md` blocker 1).
/// The coordinate/depth conventions are the least-verifiable part of the pipeline without a headset,
/// so this is a runtime tweakable rather than a compile-time choice.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ProjectionConvention {
    /// **Preferred and verified-correct.** Write a standard (non-reverse-Z) off-axis projection into
    /// `m_Projection` *before* the engine's `SetupRenderCamera`, so the engine applies its reverse-Z
    /// remap and TAA jitter to it exactly once, matching every other camera. `SetupRenderCamera`
    /// consumes the pre-written `m_Projection` in place rather than rebuilding it from FOV/near/far
    /// (settled against the engine, `docs/engine/rendering.md` §2.9), so this write reaches the GPU.
    #[default]
    EnginePreReverseZ,
    /// **Fallback / escape hatch.** Write an already-reverse-Z'd off-axis projection *after*
    /// `SetupRenderCamera` (so the engine does not re-reverse it), then rebuild the view-projections
    /// manually. TAA jitter is not applied on this path.
    ///
    /// The consume-vs-rebuild question this guarded against is now settled against the engine
    /// (`docs/engine/rendering.md` §2.9): `Camera::SetupRenderCamera` *consumes* whatever is in
    /// `m_Projection`, applying `z' = w - z` to it in place — it never rebuilds from FOV/near/far —
    /// so the pre-call [`EnginePreReverseZ`](Self::EnginePreReverseZ) write flows through correctly
    /// and is the verified-correct default. This variant is retained only as a runtime escape hatch
    /// for a headset playtest, in case the depth still reads wrong for a reason not visible from the
    /// desktop.
    ManualReverseZ,
}

/// How the per-eye blit bridges the game's captured back-buffer colour into the OpenXR swapchain.
///
/// The captured eye texture is a `CopyResource` of `m_BackBufferLinear` as `R8G8B8A8_UNORM`
/// (non-sRGB); the game presents those same bytes to a non-sRGB desktop swapchain and they look
/// correct, so the stored bytes are **display-referred** (already sRGB-encoded). The negotiated
/// OpenXR swapchain is `_SRGB`, so writing through its render-target view applies a hardware
/// linear→sRGB encode. To reproduce the original bytes the shader must therefore **linearize** the
/// sampled colour first, so the hardware re-encode cancels it out ([`Linearize`](Self::Linearize),
/// the default). If the swapchain ends up non-sRGB, or the captured content turns out to be genuine
/// linear despite the copy, [`Passthrough`](Self::Passthrough) samples and writes the colour
/// unchanged. Colours cannot be eyeballed without a headset, so this stays switchable at runtime.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BlitGamma {
    /// Decode the sampled display-referred colour to linear before writing, so the `_SRGB`
    /// render-target's hardware encode reproduces the original bytes. The correct default for a
    /// display-referred source into an `_SRGB` target.
    #[default]
    Linearize,
    /// Sample and write the colour unchanged. Correct for a genuine-linear source, or a non-sRGB
    /// target that applies no encode.
    Passthrough,
}

/// OpenXR runtime settings. `Clone` (not `Copy`) because [`loader_path`](VrConfig::loader_path) owns
/// a `String`.
#[derive(Clone, Serialize, Deserialize)]
pub struct VrConfig {
    /// Master switch: bring up the OpenXR session and render to the HMD. Off leaves the mod in
    /// flatscreen stereo (and tears any live runtime down). While on, bring-up is retried on the
    /// [`retry_interval_secs`](VrConfig::retry_interval_secs) cadence until it succeeds.
    pub enabled: bool,
    /// Per-eye swapchain resolution scale, applied to the runtime's recommended width/height. `1.0`
    /// is the runtime's recommendation; lower trades sharpness for fill rate. Clamped to a small
    /// positive minimum at swapchain creation.
    pub resolution_scale: f32,
    /// How often, in seconds, to retry OpenXR bring-up after a failure while
    /// [`enabled`](VrConfig::enabled). The mod runs in flatscreen stereo between attempts.
    pub retry_interval_secs: u64,
    /// World scale: metres of head/IPD motion per engine unit (`1.0` = 1:1). Kept here so the render wiring and the camera path share one knob.
    pub world_scale: f32,
    /// Override path to the OpenXR loader DLL. `None` loads `openxr_loader.dll` next to the payload
    /// DLL, falling back to the platform default search path.
    pub loader_path: Option<String>,
    /// Fallback near clip plane, in metres, for the per-eye off-axis projection, used only until the
    /// first camera update publishes the engine's live plane. The mod reads the active camera's actual
    /// `m_Near` each frame as the source of truth (see
    /// [`crate::hooks::camera::main_camera_planes_or`]); this default (`0.1`) mirrors the engine's
    /// `Camera` constructor value (`docs/engine/rendering.md` §2.9) so the bootstrap frame matches.
    pub near_clip: f32,
    /// Fallback far clip plane, in metres, for the per-eye off-axis projection, used only until the
    /// first camera update publishes the engine's live plane. The mod reads the active camera's actual
    /// `m_Far` each frame as the source of truth (see
    /// [`crate::hooks::camera::main_camera_planes_or`]) — the game renders a finite-far reverse-Z
    /// frustum and sets its own runtime far, so matching the live value keeps the eyes, the cull
    /// frustum, and the depth reconstruction consistent and the horizon unclipped. This default
    /// (`38400`) mirrors the engine's `Camera` constructor value (`0x47160000`) for the bootstrap frame.
    pub far_clip: f32,
    /// Which depth convention the per-eye off-axis projection is written in (see
    /// [`ProjectionConvention`]). Defaults to the preferred pre-`SetupRenderCamera` write.
    #[serde(default)]
    pub projection_convention: ProjectionConvention,
    /// How the per-eye blit bridges the captured colour into the `_SRGB` swapchain (see
    /// [`BlitGamma`]). Defaults to linearizing the display-referred capture.
    #[serde(default)]
    pub blit_srgb_gamma: BlitGamma,
    /// Render each eye at the HMD-recommended per-eye resolution (× [`resolution_scale`]) rather than
    /// the desktop display size, by driving the engine's own deferred resize (see
    /// [`crate::vr::resolution`]). On by default; disabled automatically at runtime if the resize
    /// path faults or returns the wrong size, falling back to the desktop resolution.
    ///
    /// [`resolution_scale`]: VrConfig::resolution_scale
    #[serde(default = "default_true")]
    pub native_resolution: bool,
    /// Mirror one eye to the game's own desktop window while a session is running. The engine's
    /// present stays blocked (`BLOCK_FLIP`); the mirror draws the configured eye's capture into the
    /// game swapchain's back buffer, letterboxed to the window aspect, and presents it unsynced (see
    /// [`crate::vr::mirror`]). On by default; disabled automatically at runtime on any draw/present
    /// fault, after which the game window simply shows the last mirrored (or stale) frame.
    #[serde(default = "default_true")]
    pub mirror: bool,
    /// Which eye the desktop [`mirror`](VrConfig::mirror) shows (`0` = left, `1` = right). Clamped to
    /// a valid eye at use.
    #[serde(default)]
    pub mirror_eye: u8,
    /// Persist the OpenXR **instance and session** across inject/uninject cycles instead of destroying
    /// them on teardown. The runtime allows only a small number of instances *and* sessions per
    /// process (often one each), and Proton's own startup VR probe contends for that budget, so a
    /// reinject that creates fresh ones fails with `XR_ERROR_LIMIT_REACHED`. With this on, teardown
    /// stashes both handles in the game process's environment and leaks the wrappers (the handles stay
    /// valid for the process lifetime), *without* ending the session — an ended session cannot be
    /// resumed — and a reinject re-wraps both rather than creating new ones, so VR comes back on
    /// reinject without a game relaunch. The swapchain and reference space are recreated on the reused
    /// session. On by default; a stale handle falls back to a fresh create, and a genuine stop
    /// (`enabled` off, or a lost session) destroys everything and clears the stashes.
    #[serde(default = "default_true")]
    pub persist_instance: bool,
    /// Recenter automatically when gameplay control returns to the player. Injecting VR before the
    /// scripted resume-from-menu animation (Rico standing up from the car) leaves the rig at an
    /// offset from the camera, and a fresh session's neutral is wherever the head was at session
    /// start rather than where the player actually is. With this on, the mod arms while the game is in
    /// the frontend / loading / has no local player, and fires a single [`recenter`](crate::vr::recenter)
    /// once gameplay is running and the player's head has settled (the entry animation has finished),
    /// so the neutral snaps to the player's real pose without a manual F7. It does not fire on
    /// in-session transitions like exiting a vehicle (the character stays present through those).
    #[serde(default = "default_true")]
    pub auto_recenter_on_gameplay: bool,
}

/// The serde default for [`VrConfig::native_resolution`] (the manual [`Default`] via
/// [`VrConfig::new`] is not consulted per-field when a field is absent from the serialized form).
fn default_true() -> bool {
    true
}

impl VrConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            resolution_scale: 1.0,
            retry_interval_secs: 10,
            world_scale: 1.0,
            loader_path: None,
            near_clip: 0.1,
            far_clip: 38400.0,
            projection_convention: ProjectionConvention::EnginePreReverseZ,
            blit_srgb_gamma: BlitGamma::Linearize,
            native_resolution: true,
            mirror: true,
            mirror_eye: 0,
            persist_instance: true,
            auto_recenter_on_gameplay: true,
        }
    }
}

impl Default for VrConfig {
    fn default() -> Self {
        Self::new()
    }
}
