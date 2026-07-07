//! The headpose abstraction: a pure pose source of truth.
//!
//! In flatscreen, the [`sim`] module publishes a headpose (a latching mouse-look scheme). In VR,
//! the OpenXR pose will replace the sim and publish directly here. The headpose owns no latch
//! logic, no mode detection, and no body-yaw target — those are all sim concerns. Consumers (the
//! head bone override, the camera hook, the debug UI) read the world-space pose via [`query`].
//!
//! Two auxiliary pieces of state support the consumers:
//!
//! - The **anchor**: the animated (pre-override) head bone world position, published by the
//!   character hook each frame before it overrides the bone. The pose position derives from the
//!   anchor rather than being read back from the bone — the bone reflects last frame's override,
//!   so reading it back closes a feedback loop that freezes the position at whatever world point
//!   the loop first latched onto (and drags the head bone toward it as the body walks away).
//! - The **previous pose**: the pose as of the previous input tick, rotated by [`snapshot_prev`].
//!   The engine polls input on its fixed-rate sim tick, not per rendered frame, and its camera
//!   smooths that cadence by interpolating `m_TransformT0 → m_TransformT1` with the sub-frame
//!   fraction `dtf`. Handing it the previous/current pose pair lets it smooth the headpose exactly
//!   as it smooths its own camera.
//!
//! Recentering is a source concern: the sim's pose is body-relative, so it recenters by zeroing
//! its accumulated angles ([`recenter`] delegates to [`sim::reset`]). The VR source will own its
//! own re-basing against the HMD's absolute pose when it lands (issue #12).

use std::sync::{
    OnceLock,
    atomic::{AtomicU8, Ordering},
};

use glam::{Quat, Vec3};
use parking_lot::Mutex;

pub mod config;
pub mod sim;
pub mod xr;

pub use config::HeadPoseConfig;

/// Which source currently owns the published headpose. The flatscreen [`sim`] and the VR [`xr`]
/// source are mutually exclusive publishers; the arbiter is a plain atomic so the sim's per-tick
/// gate and [`set_anchors`] can read it without taking the runtime lock (the VR frame loop holds the
/// OpenXR runtime lock across the frame, so reading it there would deadlock).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Source {
    /// The flatscreen mouse-look simulation ([`sim`]) publishes the pose.
    Sim,
    /// The OpenXR HMD source ([`xr`]) publishes the pose.
    Vr,
}

/// The active headpose source. Set once per frame by the VR frame loop (`Vr` while an OpenXR session
/// is running, else `Sim`).
static SOURCE: AtomicU8 = AtomicU8::new(Source::Sim as u8);

/// Set the active headpose source (see [`Source`]). Called once per frame by the VR frame loop.
pub fn set_source(source: Source) {
    SOURCE.store(source as u8, Ordering::Relaxed);
}

/// The active headpose source.
pub fn source() -> Source {
    if SOURCE.load(Ordering::Relaxed) == Source::Vr as u8 {
        Source::Vr
    } else {
        Source::Sim
    }
}

/// A head pose: world-space position and orientation (quaternion).
#[derive(Copy, Clone, Default)]
pub struct HeadPose {
    pub position: Vec3,
    pub orientation: Quat,
}

/// The current headpose (position + orientation), as published by the active source.
pub fn query() -> HeadPose {
    state().lock().pose
}

/// The headpose as of the previous input tick, for the engine's T0 → T1 interpolation.
pub fn query_prev() -> HeadPose {
    state().lock().prev_pose
}

/// Rotate the pose pair: the current pose becomes the previous pose. Called by the source at the
/// start of each input tick, so the previous/current pair spans exactly one tick.
pub fn snapshot_prev() {
    let mut s = state().lock();
    s.prev_pose = s.pose;
}

/// Called by the sim (or VR) to publish the headpose.
pub fn set_pose(pose: HeadPose) {
    state().lock().pose = pose;
}

/// Publish a pose to *both* sides of the interpolation pair (`pose` and `prev_pose`), so the
/// engine's `dtf` lerp degenerates to zero delta. The VR source uses this: it samples a fresh pose
/// at the predicted display time every rendered frame, so there is no tick-cadence to smooth and any
/// residual interpolation would only lag the head behind the HMD.
pub fn set_pose_no_interp(pose: HeadPose) {
    let mut s = state().lock();
    s.pose = pose;
    s.prev_pose = pose;
}

/// The animated bone positions the character hook captures each frame *before* it overrides the
/// head bone, for [`set_anchors`].
#[derive(Copy, Clone)]
pub struct Anchors {
    /// The head bone world position.
    pub head: Vec3,
    /// The neck bone world position (the camera's pitch pivot).
    pub neck: Vec3,
    /// The neck-to-eye-midpoint arm in the body frame: rotated by the published head orientation,
    /// it places the eyes anatomically about the neck pivot.
    pub eye_arm: Vec3,
}

/// Publish the animated bone anchors, captured by the character hook each frame *before* it
/// overrides the head bone. Non-finite or absurdly distant positions (loading screens,
/// uninitialized bone data) are rejected, leaving the previous anchors in place. The pose position
/// is refreshed immediately so the character hook, which reads the pose right after publishing the
/// anchors, sees a position anchored to this frame's animated pose.
pub fn set_anchors(anchors: Anchors) {
    if !anchors.head.is_finite()
        || !anchors.neck.is_finite()
        || anchors.head.length_squared() > MAX_ANCHOR_RADIUS * MAX_ANCHOR_RADIUS
        || anchors.neck.length_squared() > MAX_ANCHOR_RADIUS * MAX_ANCHOR_RADIUS
    {
        return;
    }
    let offset = crate::config::Config::lock_query(|c| c.headpose.position_offset);
    let mut s = state().lock();
    s.anchor = Some(anchors.head);
    s.neck_delta = anchors.neck - anchors.head;
    // The eye arm has its own sanity bound: a neck-to-eye distance beyond a metre is garbage bone
    // data (missing eye bones resolve to the model origin).
    if anchors.eye_arm.is_finite() && anchors.eye_arm.length_squared() < 1.0 {
        s.eye_arm = anchors.eye_arm;
    }
    // The sim's pose position is anchor-derived, so refresh it here to this frame's animated head
    // bone. The VR source owns the full pose position (the anchor plus the room-scale HMD
    // translation), so refreshing it from the anchor alone would drop that translation -- leave it
    // untouched while VR publishes.
    if source() == Source::Sim {
        s.pose.position = anchors.head + s.pose.orientation * offset;
    }
}

/// The animated head bone world position, or `None` until the character hook has published a valid
/// one (no local character yet, or only garbage bone data so far).
pub fn anchor() -> Option<Vec3> {
    state().lock().anchor
}

/// The world-space vector from the head bone anchor to the neck bone: the camera pivots the
/// configured eye offset about the neck rather than the head bone, so pitching the head swings
/// the eyes forward over the chest instead of rotating in place at the skull base. Zero until the
/// character hook has published anchors. Body-local and effectively constant between ticks, so
/// consumers may apply the current value to both sides of the pose pair.
pub fn neck_delta() -> Vec3 {
    state().lock().neck_delta
}

/// The neck-to-eye-midpoint arm in the body frame (see [`Anchors::eye_arm`]). Zero until the
/// character hook has published anchors.
pub fn eye_arm() -> Vec3 {
    state().lock().eye_arm
}

/// Recenter the headpose. The sim is body-relative and recenters by zeroing its accumulated angles;
/// the VR source re-bases the cockpit baseline against the HMD's latest located pose. Both are
/// re-based so one action recenters whichever source is live (and the other stays consistent for a
/// later switch).
pub fn recenter() {
    sim::reset();
    crate::vr::recenter();
}

/// The desired body forward on the ground plane the locomotion hook should steer the body toward,
/// or `None` to leave the body untouched. Dispatched by the active source: the VR source turns the
/// body with the look input ([`xr::body_yaw_target`]), the flatscreen sim with its head-follow latch
/// ([`sim::body_yaw_target`]).
pub fn body_yaw_target() -> Option<Vec3> {
    match source() {
        Source::Vr => xr::body_yaw_target(),
        Source::Sim => sim::body_yaw_target(),
    }
}

/// Whether headpose-driven head control is enabled.
pub fn is_active() -> bool {
    crate::config::Config::lock_query(|c| c.headpose.enabled)
}

#[derive(Default)]
struct HeadPoseState {
    /// The current headpose, written by the active source (sim or VR).
    pose: HeadPose,
    /// The headpose as of the previous input tick, for engine interpolation.
    prev_pose: HeadPose,
    /// The animated head bone world position, published by the character hook.
    anchor: Option<Vec3>,
    /// The world-space head-to-neck vector, published alongside the anchor (see [`neck_delta`]).
    neck_delta: Vec3,
    /// The body-frame neck-to-eye-midpoint arm (see [`Anchors::eye_arm`]).
    eye_arm: Vec3,
}

static STATE: OnceLock<Mutex<HeadPoseState>> = OnceLock::new();

fn state() -> &'static Mutex<HeadPoseState> {
    STATE.get_or_init(|| Mutex::new(HeadPoseState::default()))
}

/// Anchor positions beyond this radius from the origin are rejected as garbage. The world is
/// ~32 km across, so 100 km is well outside any legitimate game position.
const MAX_ANCHOR_RADIUS: f32 = 100_000.0;
