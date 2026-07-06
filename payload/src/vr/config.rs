//! VR runtime configuration. See [`crate::vr`] and `docs/vr-runtime.md`.

use serde::{Deserialize, Serialize};

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
    /// World scale: metres of head/IPD motion per engine unit (reserved for the wave-2 pose
    /// mapping; `1.0` = 1:1). Kept here so the render wiring and the camera path share one knob.
    pub world_scale: f32,
    /// Override path to the OpenXR loader DLL. `None` loads `openxr_loader.dll` next to the payload
    /// DLL, falling back to the platform default search path.
    pub loader_path: Option<String>,
    /// The near clip plane, in metres, for the per-eye off-axis projection.
    pub near_clip: f32,
    /// The far clip plane, in metres, for the per-eye off-axis projection. The engine's reverse-Z
    /// tolerates a distant far plane; the wide default suits the open world.
    pub far_clip: f32,
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
        }
    }
}

impl Default for VrConfig {
    fn default() -> Self {
        Self::new()
    }
}
