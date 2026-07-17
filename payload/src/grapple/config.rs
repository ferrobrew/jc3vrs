//! Grapple comfort configuration. Lives on [`crate::headpose::config::HeadPoseConfig`] as
//! `headpose.grapple`, so it persists and live-edits with the rest of the headpose tunables.

use serde::{Deserialize, Serialize};

/// How the body frame the head composes onto is filtered while the grapple owns the character's
/// rotation.
///
/// The player aims the grapple by looking at the target, so the head already carries the rotation
/// toward it; the grapple then rotates the body toward the same target, and the `body × head`
/// composition applies that rotation twice — the view swings past whatever was being looked at
/// (issue #36).
#[derive(Copy, Clone, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum GrappleComfortMode {
    /// No filtering; the grapple swings the view with the body.
    Off,
    /// Hold the body frame from just before the fire, so the composed view stays world-stable
    /// from fire to landing — what the player is looking at stays looked-at, and only the HMD (or
    /// mouse) moves the view. Cancels the grapple's pitch *and* yaw double-count.
    #[default]
    HoldView,
    /// Flatten the body frame to its yaw, keeping the view level. The pitch double-count is
    /// removed, but the yaw toward the target still composes; kept as an option for players who
    /// prefer the view to follow the body's heading.
    LevelPitch,
}

/// Grapple comfort settings; see [`GrappleComfortMode`] and [`crate::grapple`].
#[derive(Copy, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GrappleComfortConfig {
    /// The body-frame filter mode.
    pub mode: GrappleComfortMode,
    /// The blend-in time constant (seconds) when the filter engages. The filter holds the frame
    /// the previous render composed with, so an instant engage is seamless; a ramp here only lets
    /// the grapple's opening rotation leak through. `0` (the default) engages immediately.
    pub engage_s: f32,
    /// The blend-out time constant (seconds) when the grapple ends, easing the body rotation
    /// accumulated over the reel back into the view instead of snapping to it. Under
    /// [`yaw_handoff`](Self::yaw_handoff) this carries only the small pitch/roll residual. `0`
    /// releases immediately.
    pub release_s: f32,
    /// Hand the yaw accumulated over the reel to the *body* instead of the view (VR, on foot,
    /// hold-view only): at reel end the body-turn accumulator is aimed at the held view heading,
    /// so the character turns to face where the player is looking via the game's own rate-limited
    /// turn machinery while the hold stays engaged — the view never rotates in yaw. Falls back to
    /// the blend release on convergence or at [`handoff_timeout_s`](Self::handoff_timeout_s).
    pub yaw_handoff: bool,
    /// How long (seconds) the post-reel yaw handoff may hold the view while the body turns toward
    /// it, before falling back to the blend release (the body can be blocked from turning).
    pub handoff_timeout_s: f32,
    /// The single-step velocity change (m/s) of the body-driven head anchor beyond which the
    /// change is treated as a landing/attach snap and absorbed. The snap teleports the head up to
    /// a metre in one step (a velocity spike of 50-100+ m/s); real motion changes velocity a few
    /// m/s per step at most, so any sustained speed passes through 1:1 and only the discontinuity
    /// is absorbed (into an offset that decays over
    /// [`anchor_snap_ease_s`](Self::anchor_snap_ease_s)). The HMD's own tracking is unaffected.
    /// `0` disables the absorber.
    pub anchor_snap_threshold_mps: f32,
    /// How long (seconds) an absorbed landing snap takes to ease back out of the view.
    pub anchor_snap_ease_s: f32,
}

impl GrappleComfortConfig {
    pub const fn new() -> Self {
        Self {
            mode: GrappleComfortMode::HoldView,
            engage_s: 0.0,
            release_s: 0.4,
            yaw_handoff: true,
            handoff_timeout_s: 1.5,
            anchor_snap_threshold_mps: 25.0,
            anchor_snap_ease_s: 0.15,
        }
    }
}

impl Default for GrappleComfortConfig {
    fn default() -> Self {
        Self::new()
    }
}
