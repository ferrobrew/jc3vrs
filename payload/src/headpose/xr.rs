//! The VR headpose source: the OpenXR HMD pose, composed into the same world frame the [`sim`]
//! produces.
//!
//! [`sim`]: super::sim
//!
//! When an OpenXR session is running, this source replaces the sim as the headpose publisher (the
//! sim continues to own flatscreen). The VR runtime (`crate::vr`) locates the per-eye views each
//! frame, re-bases them into the cockpit frame against the recenter baseline, and reduces them to a
//! single **cockpit pose** -- the head's position delta and yaw-only-unrotated orientation relative
//! to the baseline. This module composes that cockpit pose into world space exactly as the sim
//! composes its body-relative offset:
//!
//! - **orientation** = body-frame rotation × cockpit orientation, so the body's pitch and roll (a
//!   banking wingsuit) carry the head with them.
//! - **position** = the animated head-bone anchor + body rotation × (cockpit position × world
//!   scale), so the HMD's room-scale translation rides the body frame. The neck/eye anatomical arm
//!   is added downstream by the camera hook (`camera_position`), reusing the anchor machinery the
//!   headpose already exposes rather than a parallel one.
//!
//! The pose is published as a tick-spaced pair ([`super::set_pose_pair`], via [`publish_pair`]): the
//! sim-driven part (the body frame and the animated head-bone anchor) advances at the engine's sim
//! tick rate, so its previous-tick and current-tick values feed the engine's `dtf` lerp and smooth
//! the sub-tick frames — without this the camera stepped at the tick rate in vehicles and while
//! parachuting, where the anchor moves ~1 m per tick. The HMD cockpit delta is sampled fresh at the
//! predicted display time every rendered frame and is identical on both sides of the pair, so it
//! passes through with zero added latency (any residual delta would only lag the head behind the
//! HMD).
//!
//! Aim is unchanged from flatscreen: the camera follows the head, and the head *is* the aim (gaze
//! aim is the interim model).
//!
//! Body yaw: the HMD owns the head, so the look effectors (mouse and right stick) no longer steer
//! it and are repurposed to turn the body ([`advance_body_yaw`]). The turned heading is handed to
//! the locomotion hook as the body's face-dir target ([`body_yaw_target`]), so the character's whole
//! frame — body and, composed onto it, the head — rotates with the input, and the native
//! turn-toward-movement never fights it (backpedaling no longer tank-turns the body).

use glam::{Quat, Vec3};
use parking_lot::Mutex;

use super::{
    HeadPose,
    config::{HeadPoseConfig, VrTurnMode},
};

/// Compose a world-space head pose from a cockpit-frame center pose and the body frame. Pure and
/// unit-testable: the caller supplies the cockpit pose (from the located views, re-based against the
/// recenter baseline), the body rotation, the animated head-bone anchor, and the world scale.
pub fn compose(
    cockpit_position: Vec3,
    cockpit_orientation: Quat,
    body_rotation: Quat,
    anchor: Vec3,
    world_scale: f32,
) -> HeadPose {
    let orientation = body_rotation * cockpit_orientation;
    let position = anchor + body_rotation * (cockpit_position * world_scale);
    HeadPose {
        position,
        orientation,
    }
}

/// Publish a composed VR pose pair (previous-tick, current-tick). Marks the VR source active and
/// writes the pair so the engine's `dtf` lerp interpolates the sim-driven camera motion (the body
/// frame and the animated head anchor, which advance at the tick rate) between ticks, while the HMD
/// cockpit delta — identical on both sides — passes through with zero added latency (see the module
/// docs).
pub fn publish_pair(prev: HeadPose, cur: HeadPose) {
    super::set_source(super::Source::Vr);
    super::set_pose_pair(prev, cur);
}

/// The HMD's head pose in the cockpit frame: its position delta and orientation relative to the
/// recenter baseline, before the body-frame composition [`compose`] applies. This is the raw
/// tracking delta the camera hook composes onto the engine's own camera when the game owns the
/// camera (loading screens, teleports), so head-tracking persists through the transition without
/// pinning the camera to the player's body (issue #27).
#[derive(Copy, Clone)]
pub struct CockpitPose {
    /// The head position relative to the recenter baseline, in the cockpit frame (metres, pre
    /// world-scale).
    pub position: Vec3,
    /// The head orientation relative to the recenter baseline.
    pub orientation: Quat,
}

/// The latest cockpit-frame HMD pose (see [`CockpitPose`]), published each rendered VR frame by
/// [`super::super::vr::begin_render_frame`]. `None` until the first VR frame renders.
static COCKPIT_POSE: Mutex<Option<CockpitPose>> = Mutex::new(None);

/// Publish the cockpit-frame HMD pose for the frame in flight (see [`CockpitPose`]). Called by the VR
/// frame loop alongside [`publish_pair`].
pub fn set_cockpit_pose(position: Vec3, orientation: Quat) {
    *COCKPIT_POSE.lock() = Some(CockpitPose {
        position,
        orientation,
    });
}

/// The latest cockpit-frame HMD pose (see [`CockpitPose`]), or `None` until the first VR frame has
/// rendered. Read by the camera hook to compose the tracking delta onto the engine camera outside
/// gameplay.
pub fn cockpit_pose() -> Option<CockpitPose> {
    *COCKPIT_POSE.lock()
}

/// Advance the VR body-yaw accumulator from this input tick's look delta, and expose the resulting
/// heading through [`body_yaw_target`]. Driven on the input tick (the game's input cadence) by
/// [`super::sim::on_input_tick`] under the VR source, since the HMD owns the head and the look
/// effectors are free to turn the body.
///
/// The accumulator is a world yaw. It is seeded from the character's current facing on the first
/// on-foot tick after a gap, so landing on foot never snaps the body, and cleared while off foot so
/// the next landing re-seeds. `look_x` follows the same sign convention as the flatscreen head yaw:
/// a rightward look turns the heading clockwise (a negative rotation about +Y).
pub fn advance_body_yaw(look_x: f32, delta_based: bool, on_foot: bool, config: &HeadPoseConfig) {
    let mut s = BODY_YAW.lock();
    if !on_foot {
        s.yaw = None;
        s.snap_armed = true;
        return;
    }

    let turn = &config.vr_turn;
    let mut yaw = s
        .yaw
        .unwrap_or_else(|| super::sim::body_yaw_of(body_rotation()));

    match turn.mode {
        VrTurnMode::Smooth => {
            if delta_based {
                // Mouse: `look_x` is a finished per-tick displacement, so add it directly at the mouse
                // scale with no deadzone (a slow mouse move is a small real delta, not stick drift).
                // Running it through the stick rate (sensitivity * smooth_scale) oversteers and the
                // deadzone drops slow motion -- the reported overshoot and stop-start.
                yaw = super::sim::wrap_angle(yaw - (look_x * turn.mouse_turn_scale).to_radians());
            } else if look_x.abs() >= turn.deadzone {
                // Stick: an absolute axis integrated as a per-tick rate.
                let delta = (look_x * config.mouse_sensitivity * turn.smooth_scale).to_radians();
                yaw = super::sim::wrap_angle(yaw - delta);
            }
        }
        VrTurnMode::Snap => {
            if s.snap_armed && look_x.abs() >= turn.snap_threshold {
                let step = look_x.signum() * turn.snap_angle_deg.to_radians();
                yaw = super::sim::wrap_angle(yaw - step);
                s.snap_armed = false;
            }
            // Re-arm the snap step once the flick relaxes, with hysteresis so one flick is one step.
            if look_x.abs() < turn.snap_threshold * 0.5 {
                s.snap_armed = true;
            }
        }
    }

    // Clamp how far the accumulated target may lead the body's current facing: the body chases the
    // target at a rate limit, so a big input jump (a mouse flick) otherwise keeps the body turning for
    // many ticks after the input stops, and once the lead passes 180° the shortest-arc catch-up
    // reverses -- the "keeps turning" and "wrong direction" reports.
    let body = super::sim::body_yaw_of(body_rotation());
    let max_lead = turn.max_body_lead_deg.max(0.0).to_radians();
    let lead = super::sim::wrap_angle(yaw - body).clamp(-max_lead, max_lead);
    yaw = super::sim::wrap_angle(body + lead);

    s.yaw = Some(yaw);
}

/// The desired body forward on the ground plane from the VR body-yaw accumulator, or `None` before
/// the accumulator has seeded (off foot, or the first on-foot tick has not run yet). Read by the
/// locomotion hook via [`super::body_yaw_target`], exactly as the flatscreen latch target is.
pub fn body_yaw_target() -> Option<Vec3> {
    BODY_YAW.lock().yaw.map(super::sim::yaw_forward)
}

/// The VR body-yaw accumulator state (see [`advance_body_yaw`]).
static BODY_YAW: Mutex<VrBodyYaw> = Mutex::new(VrBodyYaw {
    yaw: None,
    snap_armed: true,
});

struct VrBodyYaw {
    /// The accumulated world yaw (radians), or `None` while off foot / unseeded.
    yaw: Option<f32>,
    /// Whether a snap-turn step is armed (edge detection for [`VrTurnMode::Snap`]).
    snap_armed: bool,
}

/// The local player character's world rotation, the body frame the cockpit pose composes onto.
/// Identity when there is no local character (loading screens), so the head still tracks in a
/// neutral upright frame.
pub fn body_rotation() -> Quat {
    body_rotation_from(|character| glam::Mat4::from(character.m_WorldMatrixT1))
}

/// The local player character's world rotation as of the previous sim tick (`m_WorldMatrixT0`), the
/// T0 twin of [`body_rotation`]. Used to compose the previous-tick side of the VR pose pair so the
/// engine's sub-frame interpolation smooths body rotation (vehicles, parachuting) rather than
/// stepping it at the tick rate.
pub fn body_rotation_prev() -> Quat {
    body_rotation_from(|character| glam::Mat4::from(character.m_WorldMatrixT0))
}

/// Extract the local player's world rotation from the world matrix `world_of` selects. Identity when
/// there is no local character (loading screens), so the head still tracks in a neutral upright
/// frame.
fn body_rotation_from(
    world_of: impl Fn(&jc3gi::character::character::Character) -> glam::Mat4,
) -> Quat {
    unsafe {
        let character = jc3gi::character::character::Character::GetLocalPlayerCharacter();
        match character.as_ref() {
            Some(character) => {
                let (_, rotation, _) = world_of(character).to_scale_rotation_translation();
                rotation
            }
            None => Quat::IDENTITY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With an upright body, identity cockpit orientation, and unit scale, the world pose is the
    /// anchor plus the cockpit translation, oriented by the body.
    #[test]
    fn upright_body_passes_cockpit_through() {
        let anchor = Vec3::new(10.0, 2.0, -5.0);
        let cockpit_pos = Vec3::new(0.1, 0.0, -0.2);
        let pose = compose(cockpit_pos, Quat::IDENTITY, Quat::IDENTITY, anchor, 1.0);
        assert!((pose.position - (anchor + cockpit_pos)).length() < 1e-6);
        assert!((pose.orientation.dot(Quat::IDENTITY).abs() - 1.0).abs() < 1e-6);
    }

    /// The body rotation carries both the cockpit translation and orientation: a 90° body yaw
    /// rotates the cockpit's forward lean into the body's new facing.
    #[test]
    fn body_rotation_carries_cockpit() {
        let anchor = Vec3::ZERO;
        let cockpit_pos = Vec3::new(0.0, 0.0, -1.0);
        let body = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let pose = compose(cockpit_pos, Quat::IDENTITY, body, anchor, 1.0);
        // +Y yaw of 90° maps forward -Z to -X.
        assert!((pose.position - Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-5);
    }

    /// World scale scales only the translation, not the orientation.
    #[test]
    fn world_scale_scales_translation() {
        let pose = compose(
            Vec3::new(0.0, 0.0, -0.5),
            Quat::IDENTITY,
            Quat::IDENTITY,
            Vec3::ZERO,
            2.0,
        );
        assert!((pose.position - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-6);
    }
}
