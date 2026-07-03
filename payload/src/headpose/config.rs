//! Headpose configuration: the thresholds and tunables for the headpose simulation.

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// All thresholds and tunables for the headpose simulation, added to [`crate::config::Config`] as
/// `headpose`.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct HeadPoseConfig {
    /// Master switch for headpose-driven head control.
    pub enabled: bool,
    /// On-foot: the half-angle (degrees) of the decoupled cone. Beyond this, body yaw begins
    /// following the head.
    pub latch_threshold_deg: f32,
    /// On-foot: the half-angle (degrees) below which the latch disengages. Lower than
    /// [`latch_threshold_deg`](Self::latch_threshold_deg) to provide hysteresis and prevent jitter
    /// at the boundary.
    pub latch_disengage_threshold_deg: f32,
    /// Non-on-foot: maximum body-relative yaw (degrees) in either direction.
    pub free_look_yaw_limit_deg: f32,
    /// Maximum pitch (degrees) in either direction, applied in every mode: a real head cannot
    /// pitch past vertical, and letting the euler pitch cross ±90° flips the yaw/roll
    /// decomposition.
    pub free_look_pitch_limit_deg: f32,
    /// Mouse-look sensitivity (degrees per unit of look-effector delta). Whole numbers; values
    /// below 1 are too insensitive to use.
    pub mouse_sensitivity: f32,
    /// Whether to invert the Y axis (pitch).
    pub invert_y: bool,
    /// The positional offset of the head from the head bone origin, for roomscale-style testing. In
    /// metres, head-local space.
    pub position_offset: Vec3,
}
impl HeadPoseConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            latch_threshold_deg: 75.0,
            latch_disengage_threshold_deg: 65.0,
            free_look_yaw_limit_deg: 80.0,
            free_look_pitch_limit_deg: 70.0,
            mouse_sensitivity: 7.5,
            invert_y: false,
            position_offset: Vec3::ZERO,
        }
    }
}
