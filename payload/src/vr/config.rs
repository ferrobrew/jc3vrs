//! VR runtime configuration. See [`crate::vr`] and `docs/mod/vr-runtime.md`.

use serde::{Deserialize, Serialize};

/// Which depth convention the per-eye off-axis projection is written in, and where in the
/// `SetupRenderCamera` sequence it lands (`docs/engine/rendering.md` §2.7, `docs/mod/vr-runtime.md` blocker 1).
/// The coordinate/depth conventions are the least-verifiable part of the pipeline without a headset,
/// so this is a runtime tweakable rather than a compile-time choice.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ProjectionConvention {
    /// **Preferred.** Write a standard (non-reverse-Z) off-axis projection into `m_Projection`
    /// *before* the engine's `SetupRenderCamera`, so the engine applies its reverse-Z remap and TAA
    /// jitter to it exactly once, matching every other camera.
    #[default]
    EnginePreReverseZ,
    /// **Fallback.** Write an already-reverse-Z'd off-axis projection *after* `SetupRenderCamera`
    /// (so the engine does not re-reverse it), then rebuild the view-projections manually. Use this
    /// if the engine turns out to rebuild `m_Projection` from its own FOV on this path (dropping the
    /// pre-call write) or otherwise not to reverse-Z it: the depth then comes out as a thin valid
    /// wedge (§2.7's wedge bug). TAA jitter is not applied on this path.
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
    /// The near clip plane, in metres, for the per-eye off-axis projection.
    pub near_clip: f32,
    /// The far clip plane, in metres, for the per-eye off-axis projection. The engine's reverse-Z
    /// tolerates a distant far plane; the wide default suits the open world.
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
            far_clip: 4000.0,
            projection_convention: ProjectionConvention::EnginePreReverseZ,
            blit_srgb_gamma: BlitGamma::Linearize,
            native_resolution: true,
            mirror: true,
            mirror_eye: 0,
        }
    }
}

impl Default for VrConfig {
    fn default() -> Self {
        Self::new()
    }
}
