//! Far-field configuration (issue #32).

use serde::{Deserialize, Serialize};

/// The monoscopic far field (issue #32): partition each scene pass's sorted draw list into near
/// and far runs at a distance threshold, using the engine's own depth-bucket sort machinery, so
/// the far run can be skipped for dial-in today and rendered once and shared between the eyes
/// later. Off by default; see `payload/src/far_field.rs`.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FarFieldConfig {
    /// Master toggle: register the depth-bucket boundary on the passes in range and compute the
    /// near/far split (and the UI counters) each draw. Off restores each pass's stock single
    /// bucket. Off by default: `Share` mode currently produces severe compositing artifacts in
    /// VR (issue pending) and is opt-in until that is resolved.
    pub enabled: bool,
    /// The near/far boundary in metres (instance-centre distance to the sort camera). Large
    /// objects whose centre sits beyond this but whose extent reaches nearer are classified far,
    /// so keep it conservative.
    pub threshold_m: f32,
    /// The render-block type names (registry names, comma-separated) gated as inherently
    /// far-regime: their draws are skipped whenever the mode skips far, with no distance split.
    /// The volumetric terrain patches draw only distant terrain (near terrain hands off to other
    /// types as the camera approaches), and the tree impostors are the distant-tree
    /// representation; find further candidates with the Diagnostics tab's registry bisect.
    /// Under `Share`, gated types render only in the far dispatch's G-buffer range, so only
    /// opaque G-buffer types belong here — a transparent type (e.g. `Window`, the car/building
    /// glass) would vanish entirely, since its passes never run in the far dispatch.
    pub gated_types: String,
    /// What to do with the partition.
    pub mode: FarFieldMode,
}
impl FarFieldConfig {
    pub const fn new() -> Self {
        Self {
            enabled: false,
            threshold_m: 250.0,
            gated_types: String::new(),
            mode: FarFieldMode::Share,
        }
    }
}
impl Default for FarFieldConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// The default far-regime gated type list (validated by registry bisect: the volumetric terrain
/// patches and tree impostors/forest draw only the distant scene). Applied to
/// [`FarFieldConfig::gated_types`] at startup, since the const constructor cannot own a string.
pub const DEFAULT_FAR_FIELD_GATED_TYPES: &str =
    "VolumetricTerrainPatch, TreeImpostor, TerrainForest, Occluder";

/// What the far-field split does with the partition each draw.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum FarFieldMode {
    /// Compute the split and counters only; draw everything as stock.
    Collect,
    /// Skip the far run on both eyes: shows exactly what the threshold classifies as far (it
    /// vanishes), and measures the per-eye cost of the far field.
    SkipFar,
    /// Skip the near run on both eyes: shows the far field in isolation.
    SkipNear,
    /// Skip the far run on the second eye only: the sharing candidate's cost saving, with eye 1
    /// showing holes where the shared far field would composite.
    SkipFarEye1,
    /// The far-field share (issue #32 increment 2): a third, far-only dispatch renders the far field once
    /// at eye 0's pose; both near dispatches composite its captured G-buffer and render near-only
    /// on top. Requires stereo; falls back to `Collect` behaviour otherwise.
    Share,
}
