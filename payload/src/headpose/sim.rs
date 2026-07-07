//! The flatscreen headpose simulation: a latching mouse-look scheme.
//!
//! This module owns everything that is *simulation-specific*: mouse-look input handling, the
//! latch state machine, mode detection, body-yaw target computation, and the per-tick
//! [`on_input_tick`] that produces a [`HeadPose`](super::HeadPose) and publishes it via
//! [`super::set_pose`].
//!
//! The key insight: in VR, the player's head and body are independent — the head moves freely (HMD),
//! and the body is yawed via a stick. In flatscreen, we don't have independent head/body controls,
//! so the latch unifies them: within a configurable cone the head moves freely (decoupled), past
//! the threshold the body yaw tracks the head direction. This coupling is entirely a sim concern —
//! the headpose abstraction knows nothing about it.
//!
//! The accumulated yaw is **body-relative**: the head's yaw offset from the character's facing.
//! On foot, the offset is compensated when the body turns (the head stays world-anchored while the
//! body catches up to it, which is what makes the latch converge and disengage); in other modes
//! (vehicles, wingsuit) the offset rides the body frame, giving cockpit-relative free-look that
//! turns with the vehicle. The published orientation composes the offset onto the body's *full*
//! world rotation, so the body's pitch and roll carry the head with them (a banking wingsuit rolls
//! the view), and the free-look limits stay centred on the body's facing rather than on a fixed
//! world direction.
//!
//! Input arrives on the engine's fixed-rate sim tick (the device poll in
//! `InputDeviceManager::Update`), not per rendered frame. The whole sim step ([`on_input_tick`])
//! runs inside that hook, on the engine's own tick timeline: the published pose pair
//! ([`super::snapshot_prev`]) rotates at the exact moment the engine resets its sub-frame
//! interpolation fraction, so the camera hook's previous/current pair is phase-aligned with the
//! `dtf` lerp that smooths it across the frames between ticks. (Deferring the step to the next
//! frame's update left the render lerping a stale pair at `dtf ≈ 0` on every tick frame — a
//! per-tick jerk.)

use std::sync::atomic::Ordering;

use glam::{Quat, Vec3};
use parking_lot::Mutex;

use super::HeadPose;

/// The latch state (on-foot only).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum LatchState {
    /// The head moves freely within the decoupled cone; the body does not follow.
    Decoupled,
    /// The head has exceeded the latch threshold; the body is yawing to follow.
    BodyFollowing,
}

/// The current head mode, detected from the locomotion orientation-evaluator counter.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum HeadMode {
    /// On foot: the latch applies.
    OnFoot,
    /// Other (vehicle, wingsuit, etc.): free-look with clamping.
    Other,
}

/// Handle one engine input tick, called from the `InputDeviceManager::Update` hook (on the game
/// thread, inside the engine's fixed-rate sim tick): detect the mode, rotate the published pose
/// pair, integrate the look deltas, update the latch and the smoothed posture, and publish the
/// headpose. `dt` is the engine's tick delta, used for the posture low-pass.
pub fn on_input_tick(look_x: f32, look_y: f32, dt: f32) {
    let config = crate::config::Config::lock_query(|c| c.headpose);
    if !config.enabled {
        return;
    }

    let mut s = SIM.lock();

    // Mode detection, once per input tick: the locomotion orientation evaluator runs once per sim
    // tick while on foot (including idle — the move/aim task counters stop while idle), and never
    // in vehicles. This hook fires on the same sim-tick cadence, so between ticks the counter
    // advances exactly when the player is on foot. Comparing per rendered frame instead flickered
    // to `Other` on every frame without a sim tick, resetting the latch and letting the idle
    // camera pin leak through (the body tracked the head immediately).
    //
    // This runs regardless of the active source, *before* the VR early-return below: `sim::mode()`
    // is read by `head_decoupled_idle` (the body-turn suppression gate), so freezing it under VR
    // left the game's aim-relative body turn permanently unsuppressed, which the body-relative pose
    // composition (`body × cockpit`) turns into a runaway head spin.
    let evals = crate::hooks::input::locomotion::ORIENTATION_EVAL_CALLS.load(Ordering::Relaxed);
    s.mode = detect_mode(s.last_orientation_evals, evals);
    s.last_orientation_evals = evals;

    // The VR source owns the pose while an OpenXR session is running (it publishes a fresh HMD pose
    // every rendered frame). The sim must not also publish, or the two would fight over the same
    // slot; skip the rest of the tick so the VR pose stands (mode detection above still ran). The
    // look effectors do not steer the HMD-driven head, so they are consumed here to turn the body
    // instead (the flatscreen head-yaw path below owns this for the sim source).
    if super::source() == super::Source::Vr {
        super::xr::advance_body_yaw(look_x, s.mode == HeadMode::OnFoot, &config);
        return;
    }

    let body_rotation = read_body_rotation();
    let body_yaw = body_rotation.map(body_yaw_of);

    // Rotate the published pose pair on the engine's own tick timeline, phase-aligned with its
    // dtf reset (see the module docs).
    super::snapshot_prev();

    // Negated: a positive net LOOK_RIGHT delta must turn the head clockwise from above, which is
    // a negative rotation about +Y (established in-game; the unnegated form turned the wrong way).
    let yaw_delta = -(look_x * config.mouse_sensitivity).to_radians();
    let pitch_delta =
        (if config.invert_y { -look_y } else { look_y } * config.mouse_sensitivity).to_radians();

    match s.mode {
        HeadMode::OnFoot => {
            // Keep the head world-anchored while the body turns underneath it: remove the body's
            // rotation since the last tick from the body-relative offset. Without this, the offset
            // rides the turning body and the latch never converges.
            if let (Some(now), Some(then)) = (body_yaw, s.last_body_yaw) {
                s.yaw = wrap_angle(s.yaw - wrap_angle(now - then));
            }
            s.yaw = wrap_angle(s.yaw + yaw_delta);
        }
        HeadMode::Other => {
            let yaw_limit = config.free_look_yaw_limit_deg.to_radians();
            s.yaw = (s.yaw + yaw_delta).clamp(-yaw_limit, yaw_limit);
        }
    }
    // The pitch is clamped in every mode: a real head cannot pitch past vertical, and letting the
    // euler pitch cross ±90° flips the yaw/roll decomposition.
    let pitch_limit = config.free_look_pitch_limit_deg.to_radians();
    s.pitch = (s.pitch + pitch_delta).clamp(-pitch_limit, pitch_limit);
    s.last_body_yaw = body_yaw;

    match s.mode {
        HeadMode::OnFoot => {
            s.latch = update_latch(s.latch, s.yaw.to_degrees(), s.mode, &config);
            s.body_yaw_target = (s.latch == LatchState::BodyFollowing)
                .then(|| body_yaw.map(|b| yaw_forward(b + s.yaw)))
                .flatten();
        }
        HeadMode::Other => {
            s.latch = LatchState::Decoupled;
            s.body_yaw_target = None;
        }
    }

    // Publish: the head orientation rides the full body frame — the body-relative offset composed
    // onto the body's world rotation. On foot the body is upright and this reduces to a yaw
    // composition; in vehicles and the wingsuit it carries the body's pitch and roll too, so a
    // banking wingsuit rolls the view with it. Animation-driven posture (hanging, ledge grabs)
    // inverts the body in the *bone pose* over an upright root, which the root rotation alone
    // never sees, so the animated neck axis's swing away from body-up is folded in as an extra
    // posture factor — measured from the published anchors (joint translations, whose conventions
    // are proven in-game), not from joint orientations (whose rest frames are not). The position
    // anchors to the animated head bone plus the configured roomscale-testing offset.
    let posture_target = if config.posture_enabled {
        let up_body = body_rotation
            .map(|rotation| rotation.inverse() * -super::neck_delta())
            .unwrap_or(Vec3::Y);
        posture_swing(
            up_body,
            config.posture_deadband_deg,
            config.posture_full_deg,
        )
    } else {
        Quat::IDENTITY
    };
    // Low-pass the posture toward its target: the raw per-tick swing carries the walk cycle's
    // torso oscillation, and near full inversion its axis is derived from a near-zero cross
    // product and flaps tick to tick (the view's yaw spun wildly while hanging). The exponential
    // smoothing keeps only the low-frequency component — a hang settles into the inverted view
    // over the time constant, while animation-rate motion is attenuated away.
    let alpha = if config.posture_smoothing_s > f32::EPSILON {
        1.0 - (-dt.max(0.0) / config.posture_smoothing_s).exp()
    } else {
        1.0
    };
    s.posture = s.posture.slerp(posture_target, alpha).normalize();
    let head_offset = Quat::from_euler(glam::EulerRot::YXZ, s.yaw, s.pitch, s.roll);
    let orientation = body_rotation.unwrap_or(Quat::IDENTITY) * s.posture * head_offset;
    let anchor = super::anchor().unwrap_or(Vec3::ZERO);
    let position = anchor + orientation * config.position_offset;
    drop(s);
    super::set_pose(HeadPose {
        position,
        orientation,
    });
}

/// The current latch state, for UI display and the body-yaw hook.
pub fn latch_state() -> LatchState {
    SIM.lock().latch
}

/// The current head mode, for UI display and the body-yaw hook.
pub fn mode() -> HeadMode {
    SIM.lock().mode
}

/// The desired body forward on the ground plane when latched; `None` when decoupled. Read by the
/// locomotion hook, not by the headpose.
pub fn body_yaw_target() -> Option<Vec3> {
    SIM.lock().body_yaw_target
}

/// The current accumulated body-relative yaw/pitch/roll (radians), for UI display.
pub fn euler_angles() -> (f32, f32, f32) {
    let s = SIM.lock();
    (s.yaw, s.pitch, s.roll)
}

/// Reset the sim's accumulated state, re-centering the head on the body's facing. The last body
/// yaw is kept so the on-foot compensation stays continuous across the reset.
pub fn reset() {
    let mut s = SIM.lock();
    s.yaw = 0.0;
    s.pitch = 0.0;
    s.roll = 0.0;
    s.latch = LatchState::Decoupled;
    s.body_yaw_target = None;
    s.posture = Quat::IDENTITY;
}

struct SimState {
    /// Accumulated body-relative yaw (radians): the head's yaw offset from the character's facing.
    yaw: f32,
    /// Accumulated pitch (radians).
    pitch: f32,
    /// Accumulated roll (radians).
    roll: f32,
    /// The latch state (on-foot only).
    latch: LatchState,
    /// The current head mode.
    mode: HeadMode,
    /// Last frame's orientation-evaluator counter value (for mode detection).
    last_orientation_evals: u64,
    /// The body yaw at the last processed input tick, for the on-foot compensation.
    last_body_yaw: Option<f32>,
    /// The smoothed animation-posture swing (see [`posture_swing`] and the low-pass in
    /// [`on_input_tick`]).
    posture: Quat,
    /// The last computed body-yaw target, for [`body_yaw_target`].
    body_yaw_target: Option<Vec3>,
}

const SIM_DEFAULT: SimState = SimState {
    yaw: 0.0,
    pitch: 0.0,
    roll: 0.0,
    latch: LatchState::Decoupled,
    mode: HeadMode::Other,
    last_orientation_evals: 0,
    last_body_yaw: None,
    posture: Quat::IDENTITY,
    body_yaw_target: None,
};

static SIM: Mutex<SimState> = Mutex::new(SIM_DEFAULT);

/// Detect the head mode from the orientation-evaluator counter: when it advanced since the last
/// frame, the on-foot locomotion orientation executor ran, so the player is on foot.
fn detect_mode(old: u64, new: u64) -> HeadMode {
    if new > old {
        HeadMode::OnFoot
    } else {
        HeadMode::Other
    }
}

/// Update the latch state given the body-relative yaw and the current mode. On-foot uses the
/// hysteresis latch; other modes are always decoupled.
fn update_latch(
    latch: LatchState,
    relative_yaw_deg: f32,
    mode: HeadMode,
    config: &super::HeadPoseConfig,
) -> LatchState {
    if mode != HeadMode::OnFoot {
        return LatchState::Decoupled;
    }
    match latch {
        LatchState::Decoupled => {
            if relative_yaw_deg.abs() >= config.latch_threshold_deg {
                LatchState::BodyFollowing
            } else {
                LatchState::Decoupled
            }
        }
        LatchState::BodyFollowing => {
            if relative_yaw_deg.abs() <= config.latch_disengage_threshold_deg {
                LatchState::Decoupled
            } else {
                LatchState::BodyFollowing
            }
        }
    }
}

/// The animation-posture swing: the rotation taking body-up to the animated neck axis, scaled by
/// an engagement ramp. Deviations below the deadband return identity, so idle sway and locomotion
/// lean never wobble the view; between the deadband and `full_deg`, the swing ramps in; past it,
/// the full deviation applies (a hang reads as fully inverted). At exact inversion the swing axis
/// is ambiguous; body X (a pitch flip) is chosen, the common way a body ends up upside down.
fn posture_swing(up_body: Vec3, deadband_deg: f32, full_deg: f32) -> Quat {
    let Some(up) = up_body.try_normalize() else {
        return Quat::IDENTITY;
    };
    let deviation = up.dot(Vec3::Y).clamp(-1.0, 1.0).acos();
    let deadband = deadband_deg.to_radians();
    if deviation <= deadband {
        return Quat::IDENTITY;
    }
    let full = full_deg.to_radians().max(deadband + 1e-3);
    let engagement = ((deviation - deadband) / (full - deadband)).clamp(0.0, 1.0);
    let axis = Vec3::Y.cross(up);
    let axis = if axis.length_squared() > 1e-6 {
        axis.normalize()
    } else {
        Vec3::X
    };
    Quat::from_axis_angle(axis, deviation * engagement)
}

/// Wrap an angle (radians) into `[-π, π]`.
pub(super) fn wrap_angle(angle: f32) -> f32 {
    let wrapped = angle.rem_euclid(std::f32::consts::TAU);
    if wrapped > std::f32::consts::PI {
        wrapped - std::f32::consts::TAU
    } else {
        wrapped
    }
}

/// The ground-plane forward direction for a world yaw, matching the game's convention (yaw about
/// +Y, forward -Z).
pub(super) fn yaw_forward(world_yaw: f32) -> Vec3 {
    Quat::from_rotation_y(world_yaw) * Vec3::NEG_Z
}

/// Read the local player character's world rotation from `m_WorldMatrixT1`.
fn read_body_rotation() -> Option<Quat> {
    unsafe {
        let character = jc3gi::character::character::Character::GetLocalPlayerCharacter();
        let character = character.as_ref()?;
        let world = glam::Mat4::from(character.m_WorldMatrixT1);
        let (_, rotation, _) = world.to_scale_rotation_translation();
        Some(rotation)
    }
}

/// The yaw component (radians) of a body rotation, for the on-foot compensation and latch math.
pub(super) fn body_yaw_of(rotation: Quat) -> f32 {
    rotation.to_euler(glam::EulerRot::YXZ).0
}

#[cfg(test)]
mod tests;
