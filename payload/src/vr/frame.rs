//! Per-rendered-frame VR wiring: turn the located views into the headpose the camera follows and
//! the per-eye render parameters (off-axis projection + world-space eye offset) the
//! `SetupRenderCamera` hook applies.
//!
//! This is the bridge between [`crate::vr`]'s frame API (which holds the OpenXR runtime lock for the
//! duration of the frame) and the render hooks (which run during the eye Draws and must not touch
//! that lock). The parameters are extracted once at the top of the frame into a separate,
//! independently locked slot ([`RENDER_PARAMS`]) that the camera hook reads per eye. The head pose
//! is published through the [`crate::headpose::xr`] source.

use glam::Vec3;
use parking_lot::Mutex;

use crate::headpose;

use super::{FrameContext, VrConfig, config::ProjectionConvention};

/// The per-eye render parameters the `SetupRenderCamera` hook applies while a VR frame is in flight.
#[derive(Copy, Clone)]
pub struct EyeRenderParams {
    /// The standard-depth off-axis projection, to write into `m_Projection` *before*
    /// `SetupRenderCamera` under [`ProjectionConvention::EnginePreReverseZ`].
    pub projection_standard: [f32; 16],
    /// The reverse-Z off-axis projection, to write *after* `SetupRenderCamera` under
    /// [`ProjectionConvention::ManualReverseZ`].
    pub projection_reverse_z: [f32; 16],
    /// The world-space offset of this eye from the center head pose (the true per-eye delta from
    /// `locate_views`, transformed through the cockpit→body→world chain), to add to the render
    /// camera's `m_TransformF` translation.
    pub world_offset: Vec3,
    /// Which depth convention to write the projection in.
    pub convention: ProjectionConvention,
}

/// The per-eye render parameters for the frame in flight, or `None` when no VR frame is rendering
/// (flatscreen, or a VR frame the runtime asked to skip). A separate lock from the OpenXR runtime
/// state so the camera hook can read it during the eye Draws without deadlocking against the
/// frame-held runtime lock.
static RENDER_PARAMS: Mutex<Option<[EyeRenderParams; 2]>> = Mutex::new(None);

/// The render parameters for `eye` (`0` = left, `1` = right) this frame, or `None` when no VR frame
/// is rendering. Read by the `SetupRenderCamera` hook.
pub fn render_params(eye: usize) -> Option<EyeRenderParams> {
    RENDER_PARAMS.lock().as_ref().map(|p| p[eye.min(1)])
}

/// Clear the per-eye render parameters, so the camera hook falls back to flatscreen stereo. Called
/// when no VR frame renders this frame (session down, or `should_render` false).
pub fn clear_render_params() {
    *RENDER_PARAMS.lock() = None;
}

/// Drive one rendered VR frame's pose and per-eye camera parameters from the located views: reduce
/// the two eye poses to a cockpit-frame center pose, compose it into world space and publish it as
/// the headpose, and stash the per-eye off-axis projections and world offsets for the camera hook.
///
/// Only call when [`FrameContext::should_render`] is true; the located eye poses are meaningless
/// otherwise. Holds no OpenXR runtime lock beyond the borrow of `frame` (whose methods read the
/// held guard); publishes through the headpose state and the [`RENDER_PARAMS`] slot, both
/// independently locked.
pub fn begin_render_frame(frame: &FrameContext, cfg: &VrConfig) {
    let eye0 = frame.eye_view(0);
    let eye1 = frame.eye_view(1);

    let pos0 = pose_position(eye0.pose);
    let pos1 = pose_position(eye1.pose);
    let center_position = 0.5 * (pos0 + pos1);
    // The two eyes share (near enough) one orientation; take the left eye's as the cockpit
    // orientation, matching the runtime's own mid-pose stand-in.
    let center_orientation = pose_orientation(eye0.pose);

    let body_rotation = headpose::xr::body_rotation();
    let anchor = headpose::anchor().unwrap_or(Vec3::ZERO);

    let pose = headpose::xr::compose(
        center_position,
        center_orientation,
        body_rotation,
        anchor,
        cfg.world_scale,
    );
    headpose::xr::publish(pose);

    let eye_params = |eye: super::EyeView, eye_position: Vec3| EyeRenderParams {
        projection_standard: eye.projection.standard_depth,
        projection_reverse_z: eye.projection.reverse_z,
        // The eye's offset from the center head pose, in the cockpit frame, rotated into world
        // space by the body frame -- the true per-eye parallax delta, replacing the synthetic
        // ±IPD/2 lateral offset.
        world_offset: body_rotation * ((eye_position - center_position) * cfg.world_scale),
        convention: cfg.projection_convention,
    };

    *RENDER_PARAMS.lock() = Some([eye_params(eye0, pos0), eye_params(eye1, pos1)]);
}

/// The position of an OpenXR pose as a [`Vec3`].
fn pose_position(pose: openxr::Posef) -> Vec3 {
    Vec3::new(pose.position.x, pose.position.y, pose.position.z)
}

/// The orientation of an OpenXR pose as a [`glam::Quat`].
fn pose_orientation(pose: openxr::Posef) -> glam::Quat {
    glam::Quat::from_xyzw(
        pose.orientation.x,
        pose.orientation.y,
        pose.orientation.z,
        pose.orientation.w,
    )
}
