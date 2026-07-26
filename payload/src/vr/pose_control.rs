//! The frozen-pose diagnostic: pin the rendered head pose, then drive it by hand.
//!
//! While [`crate::vr::VrConfig::freeze_pose`] is on, the first frame's located eye views (plus the
//! sim-driven body frame and head anchor) are captured and reused every frame, so the rendered camera
//! is bit-identical frame to frame. On top of that capture this module holds an **editable centre
//! pose** -- position plus yaw/pitch/roll -- so a pose can be dialled in exactly and returned to. That
//! turns "does this content slide when I turn my head?" into a repeatable experiment: freeze, note the
//! numbers, nudge yaw by exactly one degree, compare.
//!
//! The authoritative frozen state is the **euler triple and position**, not a quaternion: nudging edits
//! the angles directly and the quaternion is derived from them each frame, so repeated nudges cannot
//! accumulate a quaternion→euler→quaternion round-trip error or flip through a gimbal branch. The
//! angles follow the same convention as the flatscreen head sim ([`crate::headpose::sim`]):
//! [`EulerRot::YXZ`] -- yaw about +Y, then pitch about +X, then roll about +Z, applied in that order,
//! in the cockpit frame (relative to the recenter baseline).
//!
//! The captured per-eye poses are kept *relative to* the captured centre, so the IPD and the display
//! canting survive an edit: each eye is re-composed as `centre ∘ eye-local`. Reducing the re-composed
//! pair back to a centre (midpoint position, slerp-mid orientation, exactly as
//! [`crate::vr::begin_render_frame`] does) returns the edited centre unchanged, so the numbers shown in
//! the UI are the ones the render camera is built from.
//!
//! Only [`EyeView::pose`] is edited. [`EyeView::raw_pose`] -- the compositor submission pose -- is left
//! alone because the submit reads it from the live frame data, never from the frozen copy; the runtime
//! therefore keeps reprojecting to where the head actually is, as it already did with the plain freeze.

use glam::{EulerRot, Quat, Vec3};
use parking_lot::Mutex;

use super::EyeView;

/// The euler convention for the frozen pose: yaw about +Y, then pitch about +X, then roll about +Z.
/// Matches the flatscreen head sim's composition (`headpose::sim`), so a yaw/pitch/roll reading means
/// the same thing on both paths.
pub const EULER: EulerRot = EulerRot::YXZ;

/// A head pose as the pose-control UI holds it: position in metres and orientation as degrees of
/// yaw/pitch/roll under [`EULER`], in the cockpit frame (relative to the recenter baseline).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct PoseValues {
    pub position: Vec3,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub roll_deg: f32,
}

impl PoseValues {
    /// Decompose a pose into position and euler angles under [`EULER`].
    pub fn from_pose(position: Vec3, orientation: Quat) -> Self {
        let (yaw, pitch, roll) = orientation.to_euler(EULER);
        Self {
            position,
            yaw_deg: yaw.to_degrees(),
            pitch_deg: pitch.to_degrees(),
            roll_deg: roll.to_degrees(),
        }
    }

    /// The orientation these angles describe, under [`EULER`].
    pub fn orientation(&self) -> Quat {
        Quat::from_euler(
            EULER,
            self.yaw_deg.to_radians(),
            self.pitch_deg.to_radians(),
            self.roll_deg.to_radians(),
        )
    }
}

/// The frozen per-frame inputs the VR frame loop uses in place of the live ones: the two eye views
/// (re-composed around the edited centre pose), the body frame, and the head anchor.
#[derive(Copy, Clone)]
pub struct FrozenFrame {
    pub eyes: [EyeView; 2],
    pub body_rotation: Quat,
    pub anchor: Vec3,
}

/// The frozen pose for this frame, capturing the current one via `capture` (the live eye views, body
/// rotation, and anchor) if nothing is captured yet -- so enabling the freeze pins the pose you were
/// looking from. The returned eye views carry the edited centre pose with each eye's captured offset
/// and canting re-applied.
pub fn frozen_frame(capture: impl FnOnce() -> ([EyeView; 2], Quat, Vec3)) -> FrozenFrame {
    let mut state = STATE.lock();
    state
        .get_or_insert_with(|| FrozenState::capture(capture()))
        .frame()
}

/// Drop the capture, so the next freeze re-captures the then-current pose and the UI falls back to
/// reporting the live pose.
pub fn clear() {
    *STATE.lock() = None;
}

/// The edited pose currently driving the render, or `None` when nothing is frozen (the freeze is off,
/// or no VR frame has rendered since it was turned on).
pub fn current() -> Option<PoseValues> {
    STATE.lock().as_ref().map(|s| s.current)
}

/// The pose captured at freeze time, the base [`reset`] returns to. `None` when nothing is frozen.
pub fn base() -> Option<PoseValues> {
    STATE.lock().as_ref().map(|s| s.base)
}

/// Replace the edited pose. No-op when nothing is frozen, so a stale UI edit cannot revive a capture.
pub fn set_current(values: PoseValues) {
    if let Some(state) = STATE.lock().as_mut() {
        state.current = values;
    }
}

/// Return the edited pose to the one captured when the freeze engaged, so an experiment can be
/// repeated from a known base. No-op when nothing is frozen.
pub fn reset() {
    if let Some(state) = STATE.lock().as_mut() {
        state.current = state.base;
    }
}

/// Step `value` by `steps` increments of `step`, snapped to the `step` grid.
///
/// Snapping is what makes a nudge exact and repeatable: the result is always an integer multiple of
/// the step, so `n` presses of `+1 step` land on exactly the same number as one press of a step `n`
/// times larger, and stepping back down returns to the number you left. A hand-typed value off the
/// grid is pulled onto it by the first nudge. A non-finite or non-positive step leaves the value
/// alone.
pub fn nudge(value: f32, step: f32, steps: i32) -> f32 {
    if !step.is_finite() || step <= 0.0 || !value.is_finite() {
        return value;
    }
    ((value / step).round() + steps as f32) * step
}

/// The position of an OpenXR pose as a [`Vec3`].
pub(super) fn pose_position(pose: openxr::Posef) -> Vec3 {
    Vec3::new(pose.position.x, pose.position.y, pose.position.z)
}

/// The orientation of an OpenXR pose as a [`Quat`].
pub(super) fn pose_orientation(pose: openxr::Posef) -> Quat {
    Quat::from_xyzw(
        pose.orientation.x,
        pose.orientation.y,
        pose.orientation.z,
        pose.orientation.w,
    )
}

/// An OpenXR pose from a position and orientation.
fn posef(position: Vec3, orientation: Quat) -> openxr::Posef {
    openxr::Posef {
        position: openxr::Vector3f {
            x: position.x,
            y: position.y,
            z: position.z,
        },
        orientation: openxr::Quaternionf {
            x: orientation.x,
            y: orientation.y,
            z: orientation.z,
            w: orientation.w,
        },
    }
}

/// The captured pose and the edits layered on it (`None` while the freeze is off).
static STATE: Mutex<Option<FrozenState>> = Mutex::new(None);

#[derive(Copy, Clone)]
struct FrozenState {
    /// The captured eye views, for their FOVs and projections; their poses are rebuilt per frame.
    eyes: [EyeView; 2],
    /// Each eye's pose in the captured centre's local frame (position offset, orientation delta), so
    /// the IPD and the display canting ride the edited centre.
    eye_local: [(Vec3, Quat); 2],
    /// The sim-driven body frame at capture time.
    body_rotation: Quat,
    /// The animated head-bone anchor at capture time.
    anchor: Vec3,
    /// The centre pose as captured, the base [`reset`] returns to.
    base: PoseValues,
    /// The centre pose in force, as edited.
    current: PoseValues,
}

impl FrozenState {
    fn capture((eyes, body_rotation, anchor): ([EyeView; 2], Quat, Vec3)) -> Self {
        let positions = eyes.map(|e| pose_position(e.pose));
        let orientations = eyes.map(|e| pose_orientation(e.pose));
        let centre_position = 0.5 * (positions[0] + positions[1]);
        let centre_orientation = orientations[0].slerp(orientations[1], 0.5);
        let inverse = centre_orientation.inverse();
        let eye_local = [0, 1].map(|i| {
            (
                inverse * (positions[i] - centre_position),
                inverse * orientations[i],
            )
        });
        let base = PoseValues::from_pose(centre_position, centre_orientation);
        Self {
            eyes,
            eye_local,
            body_rotation,
            anchor,
            base,
            current: base,
        }
    }

    fn frame(&self) -> FrozenFrame {
        let centre_orientation = self.current.orientation();
        let centre_position = self.current.position;
        let mut eyes = self.eyes;
        for (eye, (local_position, local_orientation)) in eyes.iter_mut().zip(self.eye_local) {
            eye.pose = posef(
                centre_position + centre_orientation * local_position,
                centre_orientation * local_orientation,
            );
        }
        FrozenFrame {
            eyes,
            body_rotation: self.body_rotation,
            anchor: self.anchor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Angles round-trip through the quaternion away from the gimbal poles.
    #[test]
    fn euler_round_trips() {
        let values = PoseValues {
            position: Vec3::new(1.0, -2.0, 0.5),
            yaw_deg: 37.0,
            pitch_deg: -12.0,
            roll_deg: 5.0,
        };
        let back = PoseValues::from_pose(values.position, values.orientation());
        assert!((back.yaw_deg - values.yaw_deg).abs() < 1e-3);
        assert!((back.pitch_deg - values.pitch_deg).abs() < 1e-3);
        assert!((back.roll_deg - values.roll_deg).abs() < 1e-3);
    }

    /// Repeatedly nudging the *angles* cannot drift, because the angles are authoritative and the
    /// quaternion is derived: 360 one-degree yaw steps land exactly back on the start.
    #[test]
    fn repeated_yaw_nudges_do_not_drift() {
        let mut yaw = 0.0f32;
        for _ in 0..360 {
            yaw = nudge(yaw, 1.0, 1);
        }
        assert_eq!(yaw, 360.0);
    }

    /// A nudge is exact on the grid: n single steps equal one n-times-larger step, in both directions.
    #[test]
    fn nudges_are_exact_and_repeatable() {
        let mut value = 0.0f32;
        for _ in 0..5 {
            value = nudge(value, 0.1, 1);
        }
        assert_eq!(value, nudge(0.0, 0.5, 1));
        for _ in 0..5 {
            value = nudge(value, 0.1, -1);
        }
        assert_eq!(value, 0.0);
    }

    /// A hand-typed value off the grid is snapped onto it by the first nudge, and stays exact after.
    #[test]
    fn nudge_snaps_off_grid_values() {
        assert_eq!(nudge(0.37, 0.1, 1), nudge(0.4, 0.1, 1));
        assert_eq!(nudge(nudge(0.37, 0.1, 1), 0.1, -1), nudge(0.4, 0.1, 0));
    }

    /// A degenerate step leaves the value untouched rather than producing a NaN.
    #[test]
    fn degenerate_step_is_inert() {
        assert_eq!(nudge(1.25, 0.0, 3), 1.25);
        assert_eq!(nudge(1.25, f32::NAN, 3), 1.25);
        assert_eq!(nudge(1.25, -0.1, 3), 1.25);
    }

    /// Re-composing the eyes around the edited centre and reducing them back the way the frame loop
    /// does (midpoint position, slerp-mid orientation) returns the edited centre, and preserves the
    /// IPD and the per-eye canting.
    #[test]
    fn recomposed_eyes_reduce_to_the_edited_centre() {
        let cant = 5f32.to_radians();
        let eye = |x: f32, cant: f32| EyeView {
            pose: posef(Vec3::new(x, 1.6, -0.2), Quat::from_rotation_y(cant)),
            raw_pose: openxr::Posef::IDENTITY,
            fov: openxr::Fovf {
                angle_left: -1.0,
                angle_right: 1.0,
                angle_up: 1.0,
                angle_down: -1.0,
            },
            projection: super::super::OffAxisProjection::new(
                super::super::Fov {
                    left: -1.0,
                    right: 1.0,
                    up: 1.0,
                    down: -1.0,
                },
                0.1,
                100.0,
            ),
        };
        let captured = (
            [eye(-0.032, cant), eye(0.032, -cant)],
            Quat::IDENTITY,
            Vec3::ZERO,
        );
        let mut state = FrozenState::capture(captured);
        state.current.yaw_deg = 30.0;
        state.current.pitch_deg = -10.0;
        state.current.position = Vec3::new(2.0, 1.0, -3.0);

        let frame = state.frame();
        let positions = frame.eyes.map(|e| pose_position(e.pose));
        let orientations = frame.eyes.map(|e| pose_orientation(e.pose));
        let centre_position = 0.5 * (positions[0] + positions[1]);
        let centre_orientation = orientations[0].slerp(orientations[1], 0.5);
        let reduced = PoseValues::from_pose(centre_position, centre_orientation);

        assert!((reduced.position - state.current.position).length() < 1e-5);
        assert!((reduced.yaw_deg - 30.0).abs() < 1e-2);
        assert!((reduced.pitch_deg + 10.0).abs() < 1e-2);
        assert!((reduced.roll_deg).abs() < 1e-2);
        // The IPD survives the edit.
        assert!(((positions[1] - positions[0]).length() - 0.064).abs() < 1e-5);
        // So does the inter-eye canting.
        let inter_eye = orientations[0].inverse() * orientations[1];
        assert!((inter_eye.to_euler(EULER).0 + 2.0 * cant).abs() < 1e-4);
    }
}
