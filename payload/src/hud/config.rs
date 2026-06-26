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
    /// HUD aspect ratio (width / height); `1.0` is square. This is the single source of truth for
    /// the HUD's aspect: the render-target dimensions, the floating panel's shape, the marker
    /// projection ([`compute_panel_vp`](crate::hud::compute_panel_vp)), and the Scaleform viewport
    /// all derive from it, so they cannot drift out of sync.
    pub aspect: f32,
    /// HUD render-target scale relative to the game's largest back-buffer axis. The texture's longer
    /// axis is `render_scale * max(back_buffer_width, back_buffer_height)` pixels; the shorter axis
    /// follows from [`aspect`](HudConfig::aspect). Lower trades sharpness for fill rate.
    pub render_scale: f32,
    /// Distance from the eye to the panel, in meters. Comfort band: 1.5-2.5m.
    pub distance: f32,
    /// Panel height in meters; the width follows from [`aspect`](HudConfig::aspect).
    pub panel_height: f32,
    /// Lazy-follow damping parameters for the floating panel.
    pub follow: FollowConfig,
}
impl HudConfig {
    pub const fn new() -> Self {
        Self {
            redirect: true,
            quad: true,
            aspect: 1.0,
            render_scale: 1.0,
            distance: 3.0,
            panel_height: 5.0,
            follow: FollowConfig::new(),
        }
    }
}

/// Lazy-follow damping parameters for the floating HUD panel. See `docs/hud.md`.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct FollowConfig {
    /// Rotation follow halflife in seconds. Lower = snappier follow.
    pub rotation_halflife: f32,
    /// Position de-jitter halflife in seconds.
    pub position_halflife: f32,
}
impl FollowConfig {
    pub const fn new() -> Self {
        Self {
            rotation_halflife: 0.2,
            position_halflife: 0.1,
        }
    }
}
