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

use std::sync::OnceLock;

use glam::{Mat4, Quat, Vec3};
use parking_lot::Mutex;

pub mod config;
pub mod sim;

pub use config::HeadPoseConfig;

/// A head pose: world-space position and orientation (quaternion).
#[derive(Copy, Clone, Default)]
pub struct HeadPose {
    pub position: Vec3,
    pub orientation: Quat,
}

impl HeadPose {
    /// Build a 4x4 world transform from this pose.
    pub fn to_mat4(self) -> Mat4 {
        Mat4::from_rotation_translation(self.orientation, self.position)
    }
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

/// Publish the animated head bone world position, captured by the character hook each frame
/// *before* it overrides the bone. Non-finite or absurdly distant positions (loading screens,
/// uninitialized bone data) are rejected, leaving the previous anchor in place. The pose position
/// is refreshed immediately so the character hook, which reads the pose right after publishing the
/// anchor, sees a position anchored to this frame's animated pose.
pub fn set_anchor(anchor: Vec3) {
    if !anchor.is_finite() || anchor.length_squared() > MAX_ANCHOR_RADIUS * MAX_ANCHOR_RADIUS {
        return;
    }
    let offset = crate::config::Config::lock_query(|c| c.headpose.position_offset);
    let mut s = state().lock();
    s.anchor = Some(anchor);
    s.pose.position = anchor + s.pose.orientation * offset;
}

/// The animated head bone world position, or `None` until the character hook has published a valid
/// one (no local character yet, or only garbage bone data so far).
pub fn anchor() -> Option<Vec3> {
    state().lock().anchor
}

/// Recenter the headpose. The sim is body-relative and recenters by zeroing its accumulated
/// angles; the VR source will re-base against the HMD pose here when it lands (issue #12).
pub fn recenter() {
    sim::reset();
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
}

static STATE: OnceLock<Mutex<HeadPoseState>> = OnceLock::new();

fn state() -> &'static Mutex<HeadPoseState> {
    STATE.get_or_init(|| Mutex::new(HeadPoseState::default()))
}

/// Anchor positions beyond this radius from the origin are rejected as garbage. The world is
/// ~32 km across, so 100 km is well outside any legitimate game position.
const MAX_ANCHOR_RADIUS: f32 = 100_000.0;
