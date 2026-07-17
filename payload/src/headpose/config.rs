//! Headpose configuration: the thresholds and tunables for the headpose simulation.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::grapple::GrappleComfortConfig;

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
    /// Non-on-foot: maximum body-relative yaw (degrees) in either direction. Ranges beyond a real
    /// head's ~80° are made anatomically plausible by the neck twist
    /// ([`neck_twist_start_deg`](Self::neck_twist_start_deg)).
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
    /// The body-relative yaw (degrees) beyond which the neck bone twists along with the head. A
    /// real head only turns so far before the neck must follow — and with the (hidden) head bone
    /// carrying the whole yaw, leaving the neck animated knots the visible neck skinning once the
    /// head turns past it.
    pub neck_twist_start_deg: f32,
    /// The maximum yaw (degrees) the neck takes: the excess beyond
    /// [`neck_twist_start_deg`](Self::neck_twist_start_deg) goes to the neck up to this cap.
    pub neck_twist_max_deg: f32,
    /// Fold the animation-driven body posture into the view. Hanging, ledge grabs, and similar
    /// authored animations invert the body in the *bone pose* over a root matrix that stays
    /// upright, so the body-frame composition alone never sees them (unlike the wingsuit, whose
    /// bank rotates the root itself). The posture is measured as the animated neck axis's swing
    /// away from body-up. Off by default: even smoothed and deadbanded, the single-axis
    /// translation-derived measurement needs more dialling in (and likely a proper bone-basis
    /// treatment) before it reads well outside of hangs — the view stays upright until then.
    pub posture_enabled: bool,
    /// Neck-axis deviations (degrees) below this are ignored, so idle sway and locomotion lean
    /// never wobble the view.
    pub posture_deadband_deg: f32,
    /// The neck-axis deviation (degrees) at which the posture is applied in full; between the
    /// deadband and this, the swing ramps in.
    pub posture_full_deg: f32,
    /// The posture low-pass time constant (seconds). The raw swing carries the walk cycle's torso
    /// oscillation and, near full inversion, a tick-to-tick axis flap; the smoothing keeps only
    /// the low-frequency component, so a hang settles into the inverted view over this constant
    /// while animation-rate motion is attenuated away. `0` disables the smoothing.
    pub posture_smoothing_s: f32,
    /// A head-local offset (metres) applied to the *whole* published headpose position — the head
    /// bone override, the camera, and the aim transform all shift together, simulating the player
    /// physically translating their head (leaning, roomscale movement). Distinct from the camera
    /// config's `head_offset`, which only moves the camera relative to the head. Stands in for the
    /// HMD's positional tracking until issue #12.
    pub position_offset: Vec3,
    /// VR on-foot body-yaw (turning) settings. In VR the HMD owns the head, so the look input
    /// (mouse and right stick) is free to turn the body instead (see [`VrTurnConfig`]).
    #[serde(default)]
    pub vr_turn: VrTurnConfig,
    /// Grapple comfort settings (see [`crate::grapple`]). Owned by the grapple module; carried
    /// here so it persists and live-edits with the rest of the headpose tunables.
    #[serde(default)]
    pub grapple: GrappleComfortConfig,
}
impl HeadPoseConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            latch_threshold_deg: 75.0,
            latch_disengage_threshold_deg: 65.0,
            free_look_yaw_limit_deg: 135.0,
            free_look_pitch_limit_deg: 70.0,
            mouse_sensitivity: 7.5,
            invert_y: false,
            neck_twist_start_deg: 60.0,
            neck_twist_max_deg: 55.0,
            posture_enabled: false,
            posture_deadband_deg: 25.0,
            posture_full_deg: 60.0,
            posture_smoothing_s: 0.5,
            position_offset: Vec3::ZERO,
            vr_turn: VrTurnConfig::new(),
            grapple: GrappleComfortConfig::new(),
        }
    }
}

/// How the VR body-yaw responds to the look input while on foot.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum VrTurnMode {
    /// Continuous rotation proportional to the look input. The most direct mapping — the mouse and
    /// stick turn the body just as they turned the head in flatscreen.
    #[default]
    Smooth,
    /// Discrete steps of [`VrTurnConfig::snap_angle_deg`], one per look-input flick past the
    /// threshold. Reduces vection for players prone to motion sickness.
    Snap,
}

/// VR on-foot body-yaw (turning) settings.
///
/// The head is HMD-driven, so the look effectors (mouse and right stick) no longer steer the head
/// and are repurposed to turn the body. The turned heading is fed to the locomotion hook as the
/// body's face-dir target, so the character's whole frame — body and, composed onto it, the head —
/// rotates with the input.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct VrTurnConfig {
    /// Smooth (continuous) or snap (discrete step) turning.
    pub mode: VrTurnMode,
    /// Smooth-turn scale: the per-tick body yaw is the look delta times
    /// [`mouse_sensitivity`](HeadPoseConfig::mouse_sensitivity) times this, so `1.0` turns the body
    /// with the look input exactly as flatscreen turned the head.
    pub smooth_scale: f32,
    /// Snap-turn step (degrees), applied once per look flick past [`snap_threshold`](Self::snap_threshold).
    pub snap_angle_deg: f32,
    /// The look-input magnitude that arms a snap step; the step re-arms once the input falls back
    /// below half this, giving hysteresis so one flick is one step.
    pub snap_threshold: f32,
    /// Look-input magnitudes below this are ignored, rejecting stick drift. Applies to *absolute*
    /// (stick) input only; delta-based (mouse) input skips it, since a slow mouse move is a small but
    /// meaningful per-tick delta, not drift.
    pub deadzone: f32,
    /// Degrees of body yaw per unit of *delta-based* (mouse) look input. The mouse LOOK effector is a
    /// per-tick displacement, not an absolute axis, so it is added directly at this scale rather than
    /// run through the stick [`smooth_scale`](Self::smooth_scale) /
    /// [`mouse_sensitivity`](HeadPoseConfig::mouse_sensitivity) rate (which oversteers on a mouse and
    /// is gated by the stick deadzone -- the stop-start, overshoot, and runaway spin).
    pub mouse_turn_scale: f32,
    /// The maximum the yaw accumulator may lead the body's current facing, in degrees. The body chases
    /// the accumulated target at a rate limit, so a big input jump (a mouse flick) otherwise pushes the
    /// target far ahead and the body keeps turning for many ticks after the input stops -- and once the
    /// lead exceeds 180° the shortest-arc catch-up reverses direction. Clamping the lead caps both.
    pub max_body_lead_deg: f32,
}

impl VrTurnConfig {
    pub const fn new() -> Self {
        Self {
            mode: VrTurnMode::Smooth,
            smooth_scale: 1.0,
            snap_angle_deg: 30.0,
            snap_threshold: 0.6,
            deadzone: 0.02,
            mouse_turn_scale: 5.0,
            max_body_lead_deg: 120.0,
        }
    }
}

impl Default for VrTurnConfig {
    fn default() -> Self {
        Self::new()
    }
}
