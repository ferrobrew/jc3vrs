//! Floating-HUD configuration types. See `docs/hud.md`.

use serde::{Deserialize, Serialize};

/// Floating-HUD settings. See `docs/hud.md`.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct HudConfig {
    /// Redirect the HUD into our own offscreen texture (the first step toward the floating panel).
    /// Off leaves the HUD on the engine surface as normal.
    pub redirect: bool,
    /// Draw the redirected HUD back into the scene as a floating quad, per eye. Requires `redirect`.
    pub quad: bool,
    /// Distance from the eye to the panel, in meters. Comfort band: 1.5-2.5m.
    pub distance: f32,
    /// Panel height in meters; width keeps the back-buffer aspect so the HUD is not distorted.
    pub panel_height: f32,
    /// Lazy-follow damping parameters for the floating panel.
    pub follow: FollowConfig,
}
impl HudConfig {
    pub const fn new() -> Self {
        Self {
            // Off by default until the redirect is proven; toggled live for first-light.
            redirect: false,
            quad: false,
            distance: 1.8,
            panel_height: 2.0,
            follow: FollowConfig::new(),
        }
    }
}

/// Lazy-follow damping parameters for the floating HUD panel. See `docs/hud.md`.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct FollowConfig {
    /// Yaw follow halflife in seconds. Lower = snappier follow.
    pub yaw_halflife: f32,
    /// Pitch follow halflife in seconds. Lower = snappier follow.
    pub pitch_halflife: f32,
    /// Roll follow halflife in seconds. Lower = snappier follow.
    pub roll_halflife: f32,
    /// Position de-jitter halflife in seconds.
    pub position_halflife: f32,
}
impl FollowConfig {
    pub const fn new() -> Self {
        Self {
            yaw_halflife: 0.15,
            pitch_halflife: 0.3,
            roll_halflife: 0.2,
            position_halflife: 0.1,
        }
    }
}
