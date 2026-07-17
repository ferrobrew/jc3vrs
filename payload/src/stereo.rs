//! Live stereo runtime state and the per-eye gating helpers shared across the render hooks.
//!
//! The Draw driver (`hooks::core::game`) maintains [`StereoState`]: it sets `active` from
//! `config.stereo.enabled` at the start of a frame and bumps `draw_index` per eye dispatch. Hooks
//! read it via [`is_second_eye`] / [`draw_index`] / [`active`]. This is *runtime* state,
//! distinct from the `stereo` config toggles in [`crate::config`].

use jc3gi::types::math::Matrix4;
use parking_lot::Mutex;

/// The live stereo render state for the frame in flight.
pub struct StereoState {
    /// Whether the current frame is being rendered in stereo (the Draw driver double-Draws).
    pub active: bool,
    /// The eye currently being drawn: 0 = first, 1 = second. During a share frame's far
    /// dispatch this stays 0 (the far phase renders at eye 0's pose).
    pub draw_index: usize,
    /// Whether the current dispatch is a share frame's far-only dispatch (issue #32): the far
    /// draw-list runs and the gated far-regime types render, everything near is skipped, and the
    /// scene/post ranges beyond the G-buffer are suppressed.
    pub far_phase: bool,
    /// Whether the current frame runs the three-dispatch far-field share (far + two near
    /// dispatches). The near dispatches window their split passes to the near run and composite
    /// the captured far G-buffer.
    pub share_frame: bool,
    /// The current dispatch's ordinal within the frame (0-based): 0/1 for plain stereo, 0/1/2 for
    /// a share frame. Once-per-frame work (the shared pre-passes) keys on `> 0`, independent of
    /// eye indexing.
    pub dispatch_ordinal: usize,
    /// The current eye's world-space camera offset from the center camera (`offset * right`, in
    /// metres), set by the `SetupRenderCamera` hook. The sun-shadow cascade correction adds
    /// `M * shadow_anchor_delta` to the cascade transform translation so the shadow lookup stays
    /// anchored at the (center-fit) shadow map instead of shifting with the per-eye camera. Zero when
    /// no per-eye offset is applied, making the correction a no-op.
    pub shadow_anchor_delta: glam::Vec3,
    /// Per-eye view-projection history for the FSR motion-vector correction, maintained by the
    /// `SetupRenderCamera` hook.
    pub vp_history: VpHistory,
    /// The render camera's center (un-offset) world transform (`m_TransformF`), snapshotted in
    /// `SetupRenderCamera` before the per-eye parallax offset is applied. The HUD panel pose reads
    /// this instead of the live `m_TransformF`, which by the time `draw_quad` runs on eye 0 already
    /// carries eye 0's half-IPD offset. Using the offset transform would shift the cached panel
    /// position toward eye 0, doubling the stereo disparity for eye 1.
    pub center_transform: Option<Matrix4>,
}
impl StereoState {
    const fn new() -> Self {
        Self {
            active: false,
            draw_index: 0,
            far_phase: false,
            share_frame: false,
            dispatch_ordinal: 0,
            shadow_anchor_delta: glam::Vec3::ZERO,
            vp_history: VpHistory::new(),
            center_transform: None,
        }
    }
}

/// One real frame of view-projection snapshots, current and previous.
///
/// The engine's velocity pass computes `curUV - prevUV` with the *per-eye* current view-projection
/// but a single, sim-side *center* previous view-projection, so in stereo every static pixel carries
/// a spurious lateral motion vector: the eye-vs-center parallax, depth-dependent and of opposite sign
/// per eye. FSR then mis-reprojects each eye's temporal history, which shows as per-eye shimmer on
/// high-contrast edges (sun-shadow boundaries especially), worst under head motion. The FSR
/// velocity-decode pass uses these snapshots to re-anchor each vector at the eye's own previous pose
/// (`fsr::velocity_decode`).
pub struct VpHistory {
    /// The previous frame's center (un-offset, unjittered) view-projection -- the matrix the engine's
    /// velocity pass reprojects with.
    pub prev_center: Option<glam::Mat4>,
    /// The previous frame's final per-eye view-projections (jitter and eye offset applied).
    pub prev_eye: [Option<glam::Mat4>; 2],
    /// This frame's center view-projection, snapshotted before the per-eye patches.
    pub cur_center: Option<glam::Mat4>,
    /// This frame's final per-eye view-projections (the matrices each dispatch rasterizes with).
    pub cur_eye: [Option<glam::Mat4>; 2],
    /// The UV-space shift the previous frame's camera jitter applied to every projected position
    /// (zero when jitter was off) -- the previous-frame half of the motion-vector jitter
    /// cancellation.
    pub prev_jitter_uv: (f32, f32),
    /// This frame's camera-jitter UV shift.
    pub cur_jitter_uv: (f32, f32),
}
impl VpHistory {
    const fn new() -> Self {
        Self {
            prev_center: None,
            prev_eye: [None, None],
            cur_center: None,
            cur_eye: [None, None],
            prev_jitter_uv: (0.0, 0.0),
            cur_jitter_uv: (0.0, 0.0),
        }
    }

    /// Advance the history one real frame: this frame's snapshots become the previous frame's.
    /// Called at the start of the eye-0 render-camera rebuild.
    pub fn rotate(&mut self) {
        self.prev_center = self.cur_center.take();
        self.prev_eye = [self.cur_eye[0].take(), self.cur_eye[1].take()];
        self.prev_jitter_uv = std::mem::take(&mut self.cur_jitter_uv);
    }

    /// The two clip-to-previous-clip reprojection matrices for `eye`: the center reprojection the
    /// engine's velocity pass encodes, and the per-eye reprojection FSR wants. `None` until a full
    /// frame of history exists. The products are formed in `f64`: the view-projections carry
    /// world-scale translations that only cancel between `prevVP` and `inv(curVP)` at
    /// higher-than-`f32` precision.
    pub fn reprojection_matrices(&self, eye: usize) -> Option<(Matrix4, Matrix4)> {
        let cur = (*self.cur_eye.get(eye)?)?;
        let prev_center = self.prev_center?;
        let prev_eye = (*self.prev_eye.get(eye)?)?;
        let inv_cur = cur.as_dmat4().inverse();
        let center_reproj = (prev_center.as_dmat4() * inv_cur).as_mat4();
        let eye_reproj = (prev_eye.as_dmat4() * inv_cur).as_mat4();
        Some((Matrix4::from(center_reproj), Matrix4::from(eye_reproj)))
    }
}

/// Global live stereo state, written by the Draw driver and read by the render hooks.
pub static STEREO_STATE: Mutex<StereoState> = Mutex::new(StereoState::new());

/// Whether the current frame is being rendered in stereo.
pub fn active() -> bool {
    STEREO_STATE.lock().active
}

/// The eye currently being drawn (0 = first, 1 = second).
pub fn draw_index() -> usize {
    STEREO_STATE.lock().draw_index
}

/// True while the Draw driver is rendering the *second* eye of a stereo frame.
pub fn is_second_eye() -> bool {
    let state = STEREO_STATE.lock();
    state.active && state.draw_index == 1
}

/// True while the Draw driver is rendering a share frame's far-only dispatch.
pub fn far_phase() -> bool {
    STEREO_STATE.lock().far_phase
}

/// True while the current frame runs the three-dispatch far-field share.
pub fn share_frame() -> bool {
    STEREO_STATE.lock().share_frame
}

/// The current dispatch's 0-based ordinal within the frame (see [`StereoState::dispatch_ordinal`]).
pub fn dispatch_ordinal() -> usize {
    STEREO_STATE.lock().dispatch_ordinal
}
