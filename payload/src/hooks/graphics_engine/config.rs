//! Graphics-engine area configuration: exposure and post-effect toggles.

use serde::{Deserialize, Serialize};

/// Auto-exposure toggles.
#[derive(Clone, Serialize, Deserialize)]
pub struct ExposureConfig {
    /// Skip the per-frame auto-exposure metering on eye 1 (the stereo-darkening fix).
    pub gate: bool,
    /// Gate `ToneMappingEffect::Update` itself to eye 0. Update advances internal ring indices used
    /// for histogram occlusion queries; running it on both eyes advances those indices twice per
    /// real frame and can read back a stale histogram once head motion makes consecutive frames
    /// diverge.
    pub gate_update: bool,
    /// Pin `m_CurrentExposure` to `forced_value` instead of the engine's auto-exposure (A/B aid).
    pub force: bool,
    /// The pinned exposure value, used when `force` is set.
    pub forced_value: f32,
}
impl ExposureConfig {
    pub const fn new() -> Self {
        Self {
            gate: true,
            gate_update: true,
            force: false,
            forced_value: 0.11,
        }
    }
}

/// Post-effect skip toggles (bisection aids / VR cleanups).
#[derive(Clone, Serialize, Deserialize)]
pub struct PostFxConfig {
    pub skip_motion_blur: bool,
    pub skip_motion_blur_recon: bool,
    pub skip_dof: bool,
    pub dof_no_reproject: bool,
    pub skip_fade: bool,
    pub skip_glare: bool,
    pub skip_player_damage: bool,
    pub skip_sun_halo: bool,
    pub skip_histogram: bool,
}
impl PostFxConfig {
    pub const fn new() -> Self {
        Self {
            skip_motion_blur: false,
            skip_motion_blur_recon: false,
            skip_dof: false,
            dof_no_reproject: true,
            skip_fade: false,
            skip_glare: false,
            skip_player_damage: false,
            skip_sun_halo: false,
            skip_histogram: false,
        }
    }
}
