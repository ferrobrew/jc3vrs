//! Coordinate-frame verification diagnostic (see `docs/mod/vr-runtime.md` "Blocker 3"): logs the render
//! camera's `m_TransformF` basis and the frame-over-frame position delta so a log reader can confirm
//! JC3's world frame before a real HMD pose is trusted. A sustained dot product of ≈ +1 between the
//! normalized travel direction and `-z_basis` while walking forward confirms right-handed, Y-up with
//! forward = −Z.

use std::time::{Duration, Instant};

use glam::Vec3;
use jc3gi::camera::camera::Camera;
use parking_lot::Mutex;
use tracing::Level;

/// The `tracing` target the diagnostic logs under. Enable it at runtime with an env filter such as
/// `RUST_LOG=coord_frame=debug`.
const TARGET: &str = "coord_frame";

/// Minimum spacing between diagnostic lines; the frame is sampled once per second, which is ample to
/// correlate a sustained walk direction with the basis without flooding the log.
const RATE_LIMIT: Duration = Duration::from_secs(1);

/// Log the render camera's `m_TransformF` basis and the position delta since the last emitted line,
/// rate-limited to [`RATE_LIMIT`]. Cheap to call when the target is filtered out: the level check
/// short-circuits before any work. Reads the basis as engine rows (row-major, row-vector): `data[0..2]`
/// right, `data[4..6]` up, `data[8..10]` +Z basis (forward = −z_basis), `data[12..14]` translation.
pub(crate) fn log_render_camera_frame(camera: &Camera) {
    if !tracing::enabled!(target: TARGET, Level::DEBUG) {
        return;
    }

    let mut state = STATE.lock();
    let now = Instant::now();
    if let Some(last) = state.last_log
        && now.duration_since(last) < RATE_LIMIT
    {
        return;
    }

    let m = &camera.m_TransformF.data;
    let position = Vec3::new(m[12], m[13], m[14]);
    let right = Vec3::new(m[0], m[1], m[2]);
    let up = Vec3::new(m[4], m[5], m[6]);
    let z_basis = Vec3::new(m[8], m[9], m[10]);

    // The velocity direction is this line's position minus the previously logged one; the reader
    // correlates its sign against the basis without needing precise timing.
    let velocity = state.last_position.map(|prev| position - prev);
    state.last_position = Some(position);
    state.last_log = Some(now);

    let (delta, dot_neg_z, dot_pos_z, dot_right, dot_up) = match velocity {
        Some(v) if v.length() > f32::EPSILON => {
            let dir = v.normalize();
            (
                v,
                dir.dot(-z_basis),
                dir.dot(z_basis),
                dir.dot(right),
                dir.dot(up),
            )
        }
        // No movement (or the first sample): the dots are undefined, so report them as zero.
        Some(v) => (v, 0.0, 0.0, 0.0, 0.0),
        None => (Vec3::ZERO, 0.0, 0.0, 0.0, 0.0),
    };

    tracing::debug!(
        target: TARGET,
        position = ?position.to_array(),
        right = ?right.to_array(),
        up = ?up.to_array(),
        z_basis = ?z_basis.to_array(),
        delta = ?delta.to_array(),
        dot_neg_z,
        dot_pos_z,
        dot_right,
        dot_up,
        "render camera m_TransformF frame; dot_neg_z ≈ +1 while walking forward confirms forward = −Z"
    );
}

/// Rate-limit and previous-position state for [`log_render_camera_frame`].
struct State {
    /// When the last diagnostic line was emitted, for rate limiting.
    last_log: Option<Instant>,
    /// The camera world position at the last emitted line, for the frame-over-frame delta.
    last_position: Option<Vec3>,
}

static STATE: Mutex<State> = Mutex::new(State {
    last_log: None,
    last_position: None,
});
