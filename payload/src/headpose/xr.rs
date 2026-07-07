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
//! The pose is published with zero interpolation delta ([`super::set_pose_no_interp`]): it is
//! sampled fresh at the predicted display time every rendered frame, so the engine's `dtf` lerp has
//! no tick cadence to smooth and any residual delta would only lag the head behind the HMD.
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

/// Publish a composed VR pose. Marks the VR source active and writes the pose with zero
/// interpolation delta (see the module docs).
pub fn publish(pose: HeadPose) {
    super::set_source(super::Source::Vr);
    super::set_pose_no_interp(pose);
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
pub fn advance_body_yaw(look_x: f32, on_foot: bool, config: &HeadPoseConfig) {
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

    if look_x.abs() >= turn.deadzone {
        match turn.mode {
            VrTurnMode::Smooth => {
                let delta = (look_x * config.mouse_sensitivity * turn.smooth_scale).to_radians();
                yaw = super::sim::wrap_angle(yaw - delta);
            }
            VrTurnMode::Snap => {
                if s.snap_armed && look_x.abs() >= turn.snap_threshold {
                    let step = look_x.signum() * turn.snap_angle_deg.to_radians();
                    yaw = super::sim::wrap_angle(yaw - step);
                    s.snap_armed = false;
                }
            }
        }
    }
    // Re-arm the snap step once the flick relaxes, with hysteresis so one flick is one step.
    if look_x.abs() < turn.snap_threshold * 0.5 {
        s.snap_armed = true;
    }

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
    unsafe {
        let character = jc3gi::character::character::Character::GetLocalPlayerCharacter();
        match character.as_ref() {
            Some(character) => {
                let world = glam::Mat4::from(character.m_WorldMatrixT1);
                let (_, rotation, _) = world.to_scale_rotation_translation();
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
