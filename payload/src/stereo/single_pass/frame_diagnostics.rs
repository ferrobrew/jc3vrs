//! The per-frame matrix snapshot the F12 screenshot writes into its JSON sidecar.
//!
//! Separate from the mechanism that produces it: nothing here is read back by the render path, so the
//! whole module exists to make a frame's `cb13` state inspectable offline rather than by squinting at
//! a capture.

use parking_lot::Mutex;

use crate::stereo::single_pass::SubstitutionStats;

/// A serializable snapshot of one eye's single-pass matrices, dumped in the F12 screenshot's JSON
/// sidecar so the exact `cb13` state can be inspected offline.
#[derive(Clone, serde::Serialize)]
pub struct EyeDiagnostics {
    /// The per-eye world position offset from the head centre (the IPD parallax), engine world units.
    pub world_offset: [f32; 3],
    /// The per-eye orientation delta from the head centre, as a quaternion `[x, y, z, w]`.
    pub orientation_delta_quat: [f32; 4],
    /// The magnitude of [`orientation_delta_quat`](Self::orientation_delta_quat), in degrees.
    pub orientation_delta_deg: f32,
    /// This eye's world-space view forward direction (`-Z` of the eye world transform).
    pub forward: [f32; 3],
    /// The per-eye reverse-Z projection from the runtime, row-major (engine `Matrix4` order).
    pub projection_reverse_z: [f32; 16],
    /// The translation-free offset view-projection written into `cb13` for this eye, row-major.
    pub cb13_view_projection: [f32; 16],
    /// The eye's world camera position written into `cb13` (`cb0[4]` equivalent).
    pub cb13_camera_position: [f32; 4],
    /// The reprojection matrix `M_eye = VP_eye · VP_center⁻¹` written into `cb13`'s `M_eye` block, one
    /// row per `cb13` row. Near-identity for a small IPD and cant; a large deviation flags a bug.
    pub cb13_m_eye: [f32; 16],
}

/// A serializable snapshot of the whole frame's single-pass matrix state, refreshed each time
/// [`compute_dual_eye_rows`] runs and dumped alongside an F12 screenshot ([`last_frame_diagnostics`]).
#[derive(Clone, serde::Serialize)]
pub struct FrameDiagnostics {
    pub single_pass: bool,
    pub dual_eye: bool,
    pub collapse: bool,
    pub double_wide: bool,
    pub capability: &'static str,
    /// The recorded full (unsplit) viewport `[x, y, w, h, minDepth, maxDepth]`, if one is bound.
    pub full_viewport: Option<[f32; 6]>,
    /// The pristine head-centre world transform, row-major (engine `Matrix4` order).
    pub center_transform: [f32; 16],
    /// The head-centre camera world position (`cb0[4]`).
    pub center_camera_position: [f32; 4],
    /// The angle between the two eyes' view forwards, in degrees (a stereo pair should be a few).
    pub forward_divergence_deg: f32,
    /// The cumulative shader-substitution tallies at capture time. See [`SubstitutionStats`].
    pub substitution: SubstitutionStats,
    pub eyes: [EyeDiagnostics; 2],
}

pub(super) static LAST_FRAME_DIAG: Mutex<Option<FrameDiagnostics>> = Mutex::new(None);

/// The most recent frame's single-pass matrix diagnostics, for the F12 screenshot JSON sidecar.
/// `None` until the dual-eye path has run at least once this session.
pub fn last_frame_diagnostics() -> Option<FrameDiagnostics> {
    LAST_FRAME_DIAG.lock().clone()
}
