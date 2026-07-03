//! The flatscreen headpose simulation: a latching mouse-look scheme.
//!
//! This module owns everything that is *simulation-specific*: mouse-look input accumulation, the
//! latch state machine, mode detection, body-yaw target computation, and the per-frame [`update`]
//! that produces a [`HeadPose`](super::HeadPose) and publishes it via [`super::set_pose`].
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
//! `InputDeviceManager::Update`), not per rendered frame, so deltas come in tick-sized bursts. The
//! sim integrates once per tick and rotates the published pose pair ([`super::snapshot_prev`]) at
//! the same cadence, so the camera hook can hand the engine a previous/current pair to interpolate
//! across the frames between ticks.

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

/// Accumulate look-effector deltas from the `InputDeviceManager::Update` hook (on the game thread).
/// Each call marks one input tick — the engine polls devices once per fixed-rate sim tick, so the
/// tick count tells [`update`] whether new input state arrived since the last frame.
pub fn accumulate_look_delta(look_x: f32, look_y: f32) {
    let mut s = SIM.lock();
    s.pending_look.0 += look_x;
    s.pending_look.1 += look_y;
    s.pending_ticks += 1;
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
    s.pending_look = (0.0, 0.0);
    s.latch = LatchState::Decoupled;
    s.body_yaw_target = None;
}

/// The per-frame step: detects the mode, integrates any input ticks, updates the latch, and
/// publishes the headpose via [`super::set_pose`]. Called from `lib::update()`.
pub fn update() {
    let config = crate::config::Config::lock_query(|c| c.headpose);
    if !config.enabled {
        return;
    }

    let mut s = SIM.lock();

    // Mode detection: the locomotion orientation evaluator runs every on-foot frame for the local
    // player (including idle), and never in vehicles. The move/aim task counters are unsuitable
    // here — those tasks stop running while idle, which read as "not on foot" and clamped the head
    // to the free-look cone whenever the player stood still.
    let evals = crate::hooks::input::locomotion::ORIENTATION_EVAL_CALLS.load(Ordering::Relaxed);
    s.mode = detect_mode(s.last_orientation_evals, evals);
    s.last_orientation_evals = evals;

    let body_rotation = read_body_rotation();
    let body_yaw = body_rotation.map(body_yaw_of);

    let (look_x, look_y) = s.pending_look;
    let ticks = s.pending_ticks;
    s.pending_look = (0.0, 0.0);
    s.pending_ticks = 0;

    if ticks > 0 {
        // A new input tick: rotate the published pose pair so the previous/current pair spans
        // exactly one tick for the engine's T0 → T1 interpolation.
        super::snapshot_prev();

        // Negated: a positive net LOOK_RIGHT delta must turn the head clockwise from above, which
        // is a negative rotation about +Y (established in-game; the unnegated form turned the
        // wrong way).
        let yaw_delta = -(look_x * config.mouse_sensitivity).to_radians();
        let pitch_delta = (if config.invert_y { -look_y } else { look_y }
            * config.mouse_sensitivity)
            .to_radians();

        match s.mode {
            HeadMode::OnFoot => {
                // Keep the head world-anchored while the body turns underneath it: remove the
                // body's rotation since the last tick from the body-relative offset. Without this,
                // the offset rides the turning body and the latch never converges.
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
        // The pitch is clamped in every mode: a real head cannot pitch past vertical, and letting
        // the euler pitch cross ±90° flips the yaw/roll decomposition.
        let pitch_limit = config.free_look_pitch_limit_deg.to_radians();
        s.pitch = (s.pitch + pitch_delta).clamp(-pitch_limit, pitch_limit);
        s.last_body_yaw = body_yaw;
    }

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
    // banking wingsuit rolls the view with it. The position anchors to the animated head bone
    // (published by the character hook pre-override) plus the configured roomscale-testing offset.
    let head_offset = Quat::from_euler(glam::EulerRot::YXZ, s.yaw, s.pitch, s.roll);
    let orientation = body_rotation.unwrap_or(Quat::IDENTITY) * head_offset;
    let anchor = super::anchor().unwrap_or(Vec3::ZERO);
    let position = anchor + orientation * config.position_offset;
    drop(s);
    super::set_pose(HeadPose {
        position,
        orientation,
    });
}

struct SimState {
    /// Accumulated body-relative yaw (radians): the head's yaw offset from the character's facing.
    yaw: f32,
    /// Accumulated pitch (radians).
    pitch: f32,
    /// Accumulated roll (radians).
    roll: f32,
    /// Pending look-effector deltas, accumulated from the input hook and consumed by [`update`].
    pending_look: (f32, f32),
    /// Input ticks since the last [`update`]: how many times the engine's device poll ran.
    pending_ticks: u32,
    /// The latch state (on-foot only).
    latch: LatchState,
    /// The current head mode.
    mode: HeadMode,
    /// Last frame's orientation-evaluator counter value (for mode detection).
    last_orientation_evals: u64,
    /// The body yaw at the last processed input tick, for the on-foot compensation.
    last_body_yaw: Option<f32>,
    /// The last computed body-yaw target, for [`body_yaw_target`].
    body_yaw_target: Option<Vec3>,
}

const SIM_DEFAULT: SimState = SimState {
    yaw: 0.0,
    pitch: 0.0,
    roll: 0.0,
    pending_look: (0.0, 0.0),
    pending_ticks: 0,
    latch: LatchState::Decoupled,
    mode: HeadMode::Other,
    last_orientation_evals: 0,
    last_body_yaw: None,
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

/// Wrap an angle (radians) into `[-π, π]`.
fn wrap_angle(angle: f32) -> f32 {
    let wrapped = angle.rem_euclid(std::f32::consts::TAU);
    if wrapped > std::f32::consts::PI {
        wrapped - std::f32::consts::TAU
    } else {
        wrapped
    }
}

/// The ground-plane forward direction for a world yaw, matching the game's convention (yaw about
/// +Y, forward -Z).
fn yaw_forward(world_yaw: f32) -> Vec3 {
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
fn body_yaw_of(rotation: Quat) -> f32 {
    rotation.to_euler(glam::EulerRot::YXZ).0
}

#[cfg(test)]
mod tests;
