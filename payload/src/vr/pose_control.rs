//! The frozen-pose diagnostic: pin the rendered pose, then drive it by hand.
//!
//! [`crate::vr::VrConfig::freeze_mode`] selects *which* pose is held:
//!
//! - [`CockpitPose`](crate::vr::FreezeMode::CockpitPose) captures the first frame's located eye views
//!   (plus the sim-driven body frame and head anchor) and reuses them every frame. That pins the
//!   **head's contribution** to the camera -- everything the HMD feeds in -- but not the camera
//!   itself: a camera the game moves still moves.
//! - [`FullCamera`](crate::vr::FreezeMode::FullCamera) captures the scene render camera's **world
//!   transform** at the last point before the engine consumes it, and rewrites it every Draw.
//!   Nothing upstream can move the view: not the head, not the body, not the animated head-bone
//!   anchor, not the game's own camera. The per-eye render parameters are held with it (see
//!   [`crate::vr::begin_render_frame`]), so the per-eye offsets do not rotate with the live head
//!   either.
//!
//! On top of whichever pose is captured this module holds an **editable centre pose** -- position
//! plus yaw/pitch/roll -- so a pose can be dialled in exactly and returned to. That turns "does this
//! content slide when the camera moves?" into a repeatable experiment: freeze, note the numbers,
//! nudge yaw by exactly one degree, compare.
//!
//! The authoritative frozen state is the **euler triple and position**, not a quaternion: nudging
//! edits the angles directly and the quaternion is derived from them each frame, so repeated nudges
//! cannot accumulate a quaternion→euler→quaternion round-trip error or flip through a gimbal
//! branch. The angles follow the same convention as the flatscreen head sim
//! ([`crate::headpose::sim`]): [`EulerRot::YXZ`] -- yaw about +Y, then pitch about +X, then roll
//! about +Z, applied in that order. The *frame* those numbers live in follows the mode: the cockpit
//! frame (relative to the recenter baseline) under the cockpit freeze, world space under the
//! full-camera one. Because the quaternion is derived from the angles, a pose whose pitch is at ±90°
//! (looking straight up or down) has no unique yaw/roll split, and the numbers there are one
//! of many equivalent readings -- the orientation is still exact, only its decomposition is
//! ambiguous.
//!
//! Under the cockpit freeze the captured per-eye poses are kept *relative to* the captured centre,
//! so the IPD and the display canting survive an edit: each eye is re-composed as
//! `centre ∘ eye-local`. Reducing the re-composed pair back to a centre (midpoint position,
//! slerp-mid orientation, exactly as [`crate::vr::begin_render_frame`] does) returns the edited
//! centre unchanged, so the numbers shown in the UI are the ones the render camera is built from.
//!
//! The compositor submission poses ([`EyeView::raw_pose`]) are latched separately by
//! [`submission_poses`] while either mode is on, so the runtime is told the image was rendered from
//! the pose it really was rendered from, and the headset view stops being reprojected toward the
//! live head.

use glam::{EulerRot, Quat, Vec3};
use parking_lot::Mutex;

use crate::vr::EyeView;

/// The euler convention for the frozen pose: yaw about +Y, then pitch about +X, then roll about +Z.
/// Matches the flatscreen head sim's composition (`headpose::sim`), so a yaw/pitch/roll reading means
/// the same thing on both paths.
pub const EULER: EulerRot = EulerRot::YXZ;

/// A pose as the pose-control UI holds it: position in metres and orientation as degrees of
/// yaw/pitch/roll under [`EULER`]. The frame is the active
/// [`FreezeMode`](crate::vr::FreezeMode)'s -- cockpit-relative or world -- so a reading only means
/// something alongside the mode it was taken in.
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

/// The frozen cockpit-frame pose for this frame
/// ([`CockpitPose`](crate::vr::FreezeMode::CockpitPose)), capturing the current one via `capture` (the
/// live eye views, body rotation, and anchor) if nothing of this kind is captured yet -- so engaging
/// the freeze pins the pose you were looking from. The returned eye views carry the edited centre
/// pose with each eye's captured offset and canting re-applied.
pub fn frozen_frame(capture: impl FnOnce() -> ([EyeView; 2], Quat, Vec3)) -> FrozenFrame {
    let mut state = STATE.lock();
    let held = match state.as_ref().and_then(|s| s.cockpit) {
        Some(cockpit) => cockpit,
        // Nothing captured, or a capture from the other mode: re-capture in this mode's frame.
        None => {
            let cockpit = CockpitCapture::capture(capture());
            *state = Some(FrozenState::new(Some(cockpit), cockpit.centre));
            cockpit
        }
    };
    let values = state.as_ref().expect("just captured").current;
    held.frame(values)
}

/// The frozen world-space render-camera transform for this frame
/// ([`FullCamera`](crate::vr::FreezeMode::FullCamera)), capturing the live one via `capture` if nothing
/// of this kind is captured yet. The returned transform is the edited one: the caller writes it
/// straight into the render camera.
///
/// The capture is decomposed to a world position + euler angles **once**, and the transform is
/// re-composed from those authoritative numbers every frame, so there is no per-frame
/// matrix→euler→matrix round trip to drift through. Re-composition is rigid (rotation + translation
/// only), which is all a camera world transform carries.
pub fn full_camera_transform(capture: impl FnOnce() -> glam::Mat4) -> glam::Mat4 {
    let mut state = STATE.lock();
    if !state.as_ref().is_some_and(|s| s.cockpit.is_none()) {
        let (_, orientation, position) = capture().to_scale_rotation_translation();
        *state = Some(FrozenState::new(
            None,
            PoseValues::from_pose(position, orientation),
        ));
    }
    let values = state.as_ref().expect("just captured").current;
    glam::Mat4::from_rotation_translation(values.orientation(), values.position)
}

/// The compositor submission poses for this frame: the live ones when `frozen` is false, else the
/// pair latched when the freeze engaged.
///
/// The submitted pose tells the runtime which viewpoint the image was rendered from, and the frozen
/// render is not from the live head -- so handing over the live pose leaves the runtime reprojecting
/// a static image toward a moving head, and the headset view warps even though the render is still.
/// Latching the pair keeps the submission consistent with the render: the frozen view sits still in
/// the world, and only the runtime's own rotational reprojection acts on it. The submission is
/// otherwise untouched (the same layer, space, and FOVs), so this cannot desynchronize the submit
/// path -- and the poses are re-latched, not accumulated, so leaving the freeze restores the live
/// pair immediately.
pub fn submission_poses(
    frozen: bool,
    capture: impl FnOnce() -> [openxr::Posef; 2],
) -> [openxr::Posef; 2] {
    let mut latched = SUBMISSION.lock();
    if !frozen {
        *latched = None;
        return capture();
    }
    *latched.get_or_insert_with(capture)
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

/// The compositor submission poses latched while a freeze is engaged (see [`submission_poses`]).
static SUBMISSION: Mutex<Option<[openxr::Posef; 2]>> = Mutex::new(None);

#[derive(Copy, Clone)]
struct FrozenState {
    /// The per-eye and sim-driven state a cockpit-frame capture needs, and the discriminant for
    /// which pose is held: `Some` is the cockpit freeze, `None` the full-camera one (whose held pose
    /// is entirely [`current`](Self::current), a world transform with nothing else to carry). Which
    /// one it is decides the frame [`base`](Self::base) and [`current`](Self::current) are in.
    cockpit: Option<CockpitCapture>,
    /// The pose as captured, the base [`reset`] returns to.
    base: PoseValues,
    /// The pose in force, as edited.
    current: PoseValues,
}

impl FrozenState {
    fn new(cockpit: Option<CockpitCapture>, base: PoseValues) -> Self {
        Self {
            cockpit,
            base,
            current: base,
        }
    }
}

#[derive(Copy, Clone)]
struct CockpitCapture {
    /// The captured eye views, for their FOVs and projections; their poses are rebuilt per frame.
    eyes: [EyeView; 2],
    /// Each eye's pose in the captured centre's local frame (position offset, orientation delta), so
    /// the IPD and the display canting ride the edited centre.
    eye_local: [(Vec3, Quat); 2],
    /// The sim-driven body frame at capture time.
    body_rotation: Quat,
    /// The animated head-bone anchor at capture time.
    anchor: Vec3,
    /// The centre pose as captured, in the cockpit frame.
    centre: PoseValues,
}

impl CockpitCapture {
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
        Self {
            eyes,
            eye_local,
            body_rotation,
            anchor,
            centre: PoseValues::from_pose(centre_position, centre_orientation),
        }
    }

    fn frame(&self, values: PoseValues) -> FrozenFrame {
        let centre_orientation = values.orientation();
        let centre_position = values.position;
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

    use crate::vr::projection::{Fov, OffAxisProjection};

    /// Serializes the tests that drive the process-wide [`STATE`], which the capture APIs share.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// A camera world transform for a test: rigid, with a non-trivial orientation well away from the
    /// gimbal poles.
    fn transform(yaw: f32, pitch: f32, roll: f32, position: Vec3) -> glam::Mat4 {
        glam::Mat4::from_rotation_translation(Quat::from_euler(EULER, yaw, pitch, roll), position)
    }

    /// An unedited full-camera freeze renders the camera it captured: the decompose-once /
    /// re-compose-per-frame path reproduces the captured transform, so engaging the freeze does not
    /// move the view.
    #[test]
    fn full_camera_capture_reproduces_the_captured_transform() {
        let _guard = TEST_LOCK.lock();
        clear();
        let captured = transform(0.9, -0.35, 0.2, Vec3::new(1234.5, 210.25, -87.75));
        let frozen = full_camera_transform(|| captured);
        assert!(frozen.abs_diff_eq(captured, 1e-3));
        // A later frame's live transform is ignored: the capture is what renders.
        let held = full_camera_transform(|| glam::Mat4::IDENTITY);
        assert!(held.abs_diff_eq(captured, 1e-3));
        clear();
    }

    /// The full-camera numbers are world-space: a one-degree yaw nudge turns the rendered camera by
    /// exactly one degree about world up and leaves its world position alone, and five one-degree
    /// steps land on the same orientation as one five-degree step.
    #[test]
    fn full_camera_nudges_are_exact_world_space_steps() {
        let _guard = TEST_LOCK.lock();
        clear();
        let position = Vec3::new(10.0, 2.0, -30.0);
        let captured = transform(0.0, 0.0, 0.0, position);
        full_camera_transform(|| captured);

        let step = |steps: i32| {
            let mut values = current().expect("frozen");
            values.yaw_deg = nudge(values.yaw_deg, 1.0, steps);
            set_current(values);
            full_camera_transform(|| glam::Mat4::IDENTITY)
        };
        for _ in 0..5 {
            step(1);
        }
        let stepped = current().expect("frozen");
        assert_eq!(stepped.yaw_deg, 5.0);
        let five_singles = full_camera_transform(|| glam::Mat4::IDENTITY);

        reset();
        let one_five = {
            let mut values = current().expect("frozen");
            values.yaw_deg = nudge(values.yaw_deg, 5.0, 1);
            set_current(values);
            full_camera_transform(|| glam::Mat4::IDENTITY)
        };
        assert!(five_singles.abs_diff_eq(one_five, 1e-6));

        let (_, orientation, moved) = five_singles.to_scale_rotation_translation();
        assert!((moved - position).length() < 1e-5);
        assert!(orientation.abs_diff_eq(Quat::from_rotation_y(5f32.to_radians()), 1e-5));

        reset();
        assert!(
            full_camera_transform(|| glam::Mat4::IDENTITY).abs_diff_eq(captured, 1e-3),
            "reset returns to the captured pose"
        );
        clear();
    }

    /// The modes are mutually exclusive: asking for the other mode's pose re-captures in that mode's
    /// frame rather than reinterpreting the held numbers in the wrong frame.
    #[test]
    fn switching_mode_recaptures_in_the_new_frame() {
        let _guard = TEST_LOCK.lock();
        clear();
        let world = transform(0.0, 0.0, 0.0, Vec3::new(500.0, 60.0, -20.0));
        full_camera_transform(|| world);
        assert_eq!(current().expect("frozen").position, world.w_axis.truncate());

        let cockpit = frozen_frame(|| (test_eyes(0.0), Quat::IDENTITY, Vec3::ZERO));
        let centre =
            0.5 * (pose_position(cockpit.eyes[0].pose) + pose_position(cockpit.eyes[1].pose));
        assert!((centre - Vec3::new(0.0, 1.6, -0.2)).length() < 1e-5);
        assert!((current().expect("frozen").position - centre).length() < 1e-5);
        clear();
    }

    /// The compositor submission poses are latched while frozen and released the moment the freeze
    /// goes off, so the submit path returns to the live pair with no residue.
    #[test]
    fn submission_poses_latch_only_while_frozen() {
        let _guard = TEST_LOCK.lock();
        let live = |x: f32| {
            [
                posef(Vec3::new(x - 0.032, 1.6, 0.0), Quat::IDENTITY),
                posef(Vec3::new(x + 0.032, 1.6, 0.0), Quat::IDENTITY),
            ]
        };
        // Unfrozen: whatever the frame located.
        assert_eq!(submission_poses(false, || live(0.0))[0].position.x, -0.032);
        // Frozen: the first frame's pair, held against a moving head.
        assert_eq!(submission_poses(true, || live(0.0))[0].position.x, -0.032);
        assert_eq!(submission_poses(true, || live(5.0))[0].position.x, -0.032);
        // Released: the live pair again, immediately.
        assert_eq!(submission_poses(false, || live(5.0))[0].position.x, 4.968);
        assert_eq!(submission_poses(true, || live(5.0))[0].position.x, 4.968);
        submission_poses(false, || live(0.0));
    }

    /// A pair of canted eye views for the tests, centred at `(0, 1.6, -0.2)` with a 64 mm IPD.
    fn test_eyes(cant: f32) -> [EyeView; 2] {
        let eye = |x: f32, cant: f32| EyeView {
            pose: posef(Vec3::new(x, 1.6, -0.2), Quat::from_rotation_y(cant)),
            raw_pose: openxr::Posef::IDENTITY,
            fov: openxr::Fovf {
                angle_left: -1.0,
                angle_right: 1.0,
                angle_up: 1.0,
                angle_down: -1.0,
            },
            projection: OffAxisProjection::new(
                Fov {
                    left: -1.0,
                    right: 1.0,
                    up: 1.0,
                    down: -1.0,
                },
                0.1,
                100.0,
            ),
        };
        [eye(-0.032, cant), eye(0.032, -cant)]
    }

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
        let captured = (test_eyes(cant), Quat::IDENTITY, Vec3::ZERO);
        let capture = CockpitCapture::capture(captured);
        let mut current = capture.centre;
        current.yaw_deg = 30.0;
        current.pitch_deg = -10.0;
        current.position = Vec3::new(2.0, 1.0, -3.0);

        let frame = capture.frame(current);
        let positions = frame.eyes.map(|e| pose_position(e.pose));
        let orientations = frame.eyes.map(|e| pose_orientation(e.pose));
        let centre_position = 0.5 * (positions[0] + positions[1]);
        let centre_orientation = orientations[0].slerp(orientations[1], 0.5);
        let reduced = PoseValues::from_pose(centre_position, centre_orientation);

        assert!((reduced.position - current.position).length() < 1e-5);
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
