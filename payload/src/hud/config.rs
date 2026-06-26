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
    /// Aspect ratio (width / height) for the gameplay HUD; `1.0` is square. The effective aspect for
    /// the current frame ([`hud_aspect`](HudConfig::hud_aspect) or [`movie_aspect`](HudConfig::movie_aspect),
    /// per the [`HudMode`](crate::hud::HudMode)) is the single source of truth for the HUD's shape:
    /// the render-target dimensions, the floating panel, the marker projection
    /// ([`compute_panel_vp`](crate::hud::compute_panel_vp)), and the Scaleform viewport all derive
    /// from it, so they cannot drift out of sync.
    pub hud_aspect: f32,
    /// Aspect ratio (width / height) for full-screen UI -- movies, loading screens, and menus
    /// ([`HudMode::Movie`](crate::hud::HudMode)); `16:9` by default. See [`hud_aspect`](HudConfig::hud_aspect).
    pub movie_aspect: f32,
    /// HUD render-target scale relative to the game's largest back-buffer axis. The texture's longer
    /// axis is `render_scale * max(back_buffer_width, back_buffer_height)` pixels; the shorter axis
    /// follows from the effective aspect. Lower trades sharpness for fill rate.
    pub render_scale: f32,
    /// Distance from the eye to the panel, in meters. The panel resizes with distance to keep a
    /// constant apparent (angular) size, so this can be changed freely without the HUD growing or
    /// shrinking. Comfort band: 1.5-3m.
    pub distance: f32,
    /// Apparent-size multiplier for the panel; `1.0` is the comfortable baseline (4 m wide at 3 m).
    /// The physical size is derived from this, [`distance`](HudConfig::distance), and the effective
    /// aspect (see [`crate::hud::panel_height`]), so changing the distance or aspect keeps the panel
    /// looking the same size and fitting the same horizontal content.
    pub panel_scale: f32,
    /// Lazy-follow damping parameters for the floating panel.
    pub follow: FollowConfig,
}
impl HudConfig {
    pub const fn new() -> Self {
        Self {
            redirect: true,
            quad: true,
            hud_aspect: 1.0,
            movie_aspect: 16.0 / 9.0,
            render_scale: 1.0,
            distance: 3.0,
            panel_scale: 1.0,
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
