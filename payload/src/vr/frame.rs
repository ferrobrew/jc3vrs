//! Per-rendered-frame VR wiring: turn the located views into the headpose the camera follows and
//! the per-eye render parameters (off-axis projection + world-space eye offset) the
//! `SetupRenderCamera` hook applies.
//!
//! This is the bridge between [`crate::vr`]'s frame API (which holds the OpenXR runtime lock for the
//! duration of the frame) and the render hooks (which run during the eye Draws and must not touch
//! that lock). The parameters are extracted once at the top of the frame into a separate,
//! independently locked slot ([`RENDER_PARAMS`]) that the camera hook reads per eye. The head pose
//! is published through the [`crate::headpose::xr`] source.

use glam::{Quat, Vec3};
use parking_lot::Mutex;

use crate::headpose;

use super::{Fov, FrameContext, OffAxisProjection, VrConfig, config::ProjectionConvention};

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
    /// This eye's orientation relative to the center head pose (the display canting from
    /// `locate_views`), as a head-local rotation. The camera hook applies it to the render camera's
    /// basis, about the already-offset eye position, so the rendered content matches the per-eye pose
    /// submitted to the compositor. Identity on parallel-panel HMDs; the Valve Index cants each eye
    /// ~5°, and dropping it leaves the two eyes rotationally mismatched, so the stereo will not fuse.
    pub orientation_delta: Quat,
    /// Which depth convention to write the projection in.
    pub convention: ProjectionConvention,
}

/// The per-eye render parameters for the frame in flight, or `None` when no VR frame is rendering
/// (flatscreen, or a VR frame the runtime asked to skip). A separate lock from the OpenXR runtime
/// state so the camera hook can read it during the eye Draws without deadlocking against the
/// frame-held runtime lock.
static RENDER_PARAMS: Mutex<Option<[EyeRenderParams; 2]>> = Mutex::new(None);

/// The standard-depth, symmetric, union-FOV projection that bounds *both* eyes' off-axis frusta, for
/// widening the scene cull frustum (see [`cull_projection_standard`]). `None` when no VR frame is
/// rendering.
static CULL_PROJECTION: Mutex<Option<[f32; 16]>> = Mutex::new(None);

/// The render parameters for `eye` (`0` = left, `1` = right) this frame, or `None` when no VR frame
/// is rendering. Read by the `SetupRenderCamera` hook.
pub fn render_params(eye: usize) -> Option<EyeRenderParams> {
    RENDER_PARAMS.lock().as_ref().map(|p| p[eye.min(1)])
}

/// The symmetric union-FOV projection covering both eyes' off-axis frusta, in the engine's
/// standard-depth `m_ProjectionF` layout, or `None` when no VR frame is rendering. The occluder-cull
/// hook writes this into the shared cull camera's `m_ProjectionF` so the visibility cull -- which the
/// engine runs once per frame against the narrower center camera -- covers everything either eye can
/// see, removing the black voids and pop-in at the outer edges.
pub fn cull_projection_standard() -> Option<[f32; 16]> {
    *CULL_PROJECTION.lock()
}

/// Clear the per-eye render parameters (and the cull projection), so the camera hook falls back to
/// flatscreen stereo. Called when no VR frame renders this frame (session down, or `should_render`
/// false).
pub fn clear_render_params() {
    *RENDER_PARAMS.lock() = None;
    *CULL_PROJECTION.lock() = None;
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
    // The true mid orientation (slerp of the two eyes), so the cockpit/head frame sits between the
    // eyes rather than on one of them. On canted panels the eyes' orientations differ, so anchoring
    // the center on one eye would bias the head (and everything keyed to it -- aim, the recenter
    // baseline) toward that eye and split the per-eye canting asymmetrically. Matches the runtime
    // head-pose stand-in ([`super::mid_pose`]), which is likewise the slerp-mid.
    let center_orientation = pose_orientation(eye0.pose).slerp(pose_orientation(eye1.pose), 0.5);

    let body_rotation = headpose::xr::body_rotation();
    let anchor = headpose::anchor().unwrap_or(Vec3::ZERO);

    // Compose a tick-spaced pose pair sharing the fresh HMD cockpit delta but differing in the
    // sim-driven body frame and head anchor (T1 vs T0), so the engine's `dtf` lerp smooths the
    // per-tick body/anchor motion between rendered frames. The previous-tick anchor and body rotation
    // fall back to the current-tick values until they are available, degenerating to no interpolation
    // rather than a bad one.
    let body_rotation_prev = headpose::xr::body_rotation_prev();
    let anchor_prev = headpose::anchor_prev().unwrap_or(anchor);

    let cur = headpose::xr::compose(
        center_position,
        center_orientation,
        body_rotation,
        anchor,
        cfg.world_scale,
    );
    let prev = headpose::xr::compose(
        center_position,
        center_orientation,
        body_rotation_prev,
        anchor_prev,
        cfg.world_scale,
    );
    headpose::xr::publish_pair(prev, cur);

    // The symmetric union-FOV cull projection: a single centred frustum that contains both eyes'
    // off-axis frusta. Each eye is laterally offset by ~IPD/2 and has its own asymmetric FOV, so the
    // superset bounds the wider of the two on each side (in tangent space) plus a near-plane margin
    // `s = (IPD/2) / near` for the lateral eye shift. `s·z` covers the shift exactly at the near plane
    // and over-covers (harmlessly) with distance, so nothing either eye can see is culled. The engine
    // culls at a single interpolated pose, so `cull_fov_padding` pads each side's tangent outward on
    // top of that to hide edge pop-in under fast motion; the eye-shift margin and the padding are both
    // applied on the vertical axis too (flying pitch shifts the eyes vertically). Written standard-depth
    // to match the cull camera's `m_ProjectionF`.
    let ipd = (pos1 - pos0).length();
    let margin = 0.5 * ipd / cfg.near_clip.max(1e-3);
    // The padding lives on the stereo config alongside its `widen_cull_frustum` sibling (and the
    // debug slider), not on `VrConfig`, so read it from the global config here.
    let pad = 1.0 + crate::config::Config::lock_query(|c| c.stereo.cull_fov_padding).max(0.0);
    // Widen each side in tangent space: scale the half-extent outward by `pad`, then push out by the
    // eye-shift margin in the side's own direction (`copysign` keeps left/down negative, right/up
    // positive). Vertical uses tangents here (not raw angles) so it receives the same treatment.
    let expand = |t: f32| t * pad + margin.copysign(t);
    let union_fov = Fov {
        left: expand(eye0.fov.angle_left.tan().min(eye1.fov.angle_left.tan())).atan(),
        right: expand(eye0.fov.angle_right.tan().max(eye1.fov.angle_right.tan())).atan(),
        up: expand(eye0.fov.angle_up.tan().max(eye1.fov.angle_up.tan())).atan(),
        down: expand(eye0.fov.angle_down.tan().min(eye1.fov.angle_down.tan())).atan(),
    };
    *CULL_PROJECTION.lock() =
        Some(OffAxisProjection::new(union_fov, cfg.near_clip, cfg.far_clip).standard_depth);

    let eye_params = |eye: super::EyeView, eye_position: Vec3| EyeRenderParams {
        projection_standard: eye.projection.standard_depth,
        projection_reverse_z: eye.projection.reverse_z,
        // The eye's offset from the center head pose, in the cockpit frame, rotated into world
        // space by the body frame -- the true per-eye parallax delta, replacing the synthetic
        // ±IPD/2 lateral offset.
        world_offset: body_rotation * ((eye_position - center_position) * cfg.world_scale),
        // The eye's orientation relative to the center head pose, in the head-local frame -- the
        // display canting. `center_orientation` is the slerp-mid, so each eye carries half the
        // inter-eye cant (symmetric); the camera hook applies it locally so each eye renders at the
        // orientation it is submitted with.
        orientation_delta: center_orientation.inverse() * pose_orientation(eye.pose),
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
