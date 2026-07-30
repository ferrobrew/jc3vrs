//! Recentering the cockpit: the baseline the per-eye poses are expressed relative to, the manual
//! [`recenter`], and the gameplay-resume auto-recenter ([`auto_recenter_tick`]).

use openxr as xr;
use parking_lot::Mutex;

use crate::{
    config::Config,
    vr::{is_running, state::VR_STATE},
};

/// Recenter the cockpit: re-base the stored baseline from the latest located VIEW-space pose, taking
/// its position and yaw only (the cockpit model). The frame loop consumes the baseline when
/// mapping per-eye poses (see [`crate::vr::frame`]). No-op until a frame has located a head pose.
pub fn recenter() {
    let mut state = VR_STATE.lock();
    match state.latest_head_pose {
        Some(pose) => {
            state.baseline = Some(Baseline::from_pose(pose));
            tracing::info!(target: "vr", "recentered the cockpit baseline");
        }
        None => {
            tracing::warn!(target: "vr", "recenter requested before any head pose was located");
        }
    }
}

/// Drive the gameplay-resume auto-recenter. Call once per frame on the game thread. Arms while the
/// game is not in settled gameplay (frontend, loading, or no local player character), and fires a
/// single [`recenter`] once gameplay is running and the player's head anchor has stopped moving (the
/// entry animation has finished), so the neutral snaps to the player's real pose. In-session
/// transitions such as exiting a vehicle keep a live character, so they never re-arm and never fire.
/// See [`crate::vr::VrConfig::auto_recenter_on_gameplay`].
pub fn auto_recenter_tick() {
    if !Config::lock_query(|c| c.vr.auto_recenter_on_gameplay) || !is_running() {
        return;
    }
    let in_gameplay = crate::hooks::in_gameplay();
    let has_character =
        unsafe { jc3gi::character::character::Character::GetLocalPlayerCharacter().as_ref() }
            .is_some();
    let on_foot = crate::headpose::sim::mode() == crate::headpose::sim::HeadMode::OnFoot;
    let anchor = crate::headpose::anchor();

    let mut s = AUTO_RECENTER.lock();
    // Arm whenever we are not in settled gameplay, so the next resume fires exactly one recenter while
    // in-session transitions (which keep a live character) do not.
    if !in_gameplay || !has_character {
        s.armed = true;
        s.settled_frames = 0;
        s.last_anchor = None;
        return;
    }
    if !s.armed {
        return;
    }
    // Wait for the head anchor to hold still (the entry animation to end) before recentering.
    match (on_foot, anchor) {
        (true, Some(a)) => {
            let settled = s
                .last_anchor
                .is_some_and(|prev| (a - prev).length() < ANCHOR_SETTLE_EPS);
            s.settled_frames = if settled { s.settled_frames + 1 } else { 0 };
            s.last_anchor = Some(a);
            if s.settled_frames >= ANCHOR_SETTLE_FRAMES {
                s.armed = false;
                drop(s);
                recenter();
                tracing::info!(target: "vr", "auto-recentered on gameplay resume");
            }
        }
        _ => {
            s.settled_frames = 0;
            s.last_anchor = anchor;
        }
    }
}

/// The recenter baseline: a position and a yaw-only orientation, re-based from the latest VIEW-space
/// pose. Per-eye poses are expressed relative to this transform (the cockpit model).
#[derive(Copy, Clone)]
pub(super) struct Baseline {
    /// The world-from-baseline transform (position + yaw). Per-eye poses are re-based by its inverse.
    position: glam::Vec3,
    /// Yaw-only orientation (rotation about the up axis).
    yaw: glam::Quat,
}

impl Baseline {
    /// Extract the position and yaw-only orientation from a located VIEW-space pose.
    fn from_pose(pose: xr::Posef) -> Self {
        let orientation = glam::Quat::from_xyzw(
            pose.orientation.x,
            pose.orientation.y,
            pose.orientation.z,
            pose.orientation.w,
        );
        // Yaw only: project the orientation onto rotation about the Y (up) axis. Zero the X/Z
        // components of the quaternion and renormalize; a degenerate (looking straight up/down)
        // quaternion falls back to identity yaw.
        let yaw = glam::Quat::from_xyzw(0.0, orientation.y, 0.0, orientation.w);
        let yaw = if yaw.length_squared() > 1e-6 {
            yaw.normalize()
        } else {
            glam::Quat::IDENTITY
        };
        Self {
            position: glam::Vec3::new(pose.position.x, pose.position.y, pose.position.z),
            yaw,
        }
    }

    /// Re-base a located pose into the baseline (cockpit) frame: `baseline⁻¹ · pose`.
    pub(super) fn rebase(&self, pose: xr::Posef) -> xr::Posef {
        let pos = glam::Vec3::new(pose.position.x, pose.position.y, pose.position.z);
        let rot = glam::Quat::from_xyzw(
            pose.orientation.x,
            pose.orientation.y,
            pose.orientation.z,
            pose.orientation.w,
        );
        let inv_yaw = self.yaw.conjugate();
        let rel_pos = inv_yaw * (pos - self.position);
        let rel_rot = inv_yaw * rot;
        xr::Posef {
            orientation: xr::Quaternionf {
                x: rel_rot.x,
                y: rel_rot.y,
                z: rel_rot.z,
                w: rel_rot.w,
            },
            position: xr::Vector3f {
                x: rel_pos.x,
                y: rel_pos.y,
                z: rel_pos.z,
            },
        }
    }
}

/// The midpoint pose of the two eyes (position averaged, orientation slerped halfway): a stand-in
/// head pose for the recenter baseline. The slerp-mid keeps the head frame between the eyes rather
/// than on one of them, matching the render center in [`crate::vr::begin_render_frame`]; on canted
/// panels (e.g. the Valve Index) the eyes' orientations differ, so taking one eye's would bias the
/// baseline.
pub(super) fn mid_pose(a: xr::Posef, b: xr::Posef) -> xr::Posef {
    let qa = glam::Quat::from_xyzw(
        a.orientation.x,
        a.orientation.y,
        a.orientation.z,
        a.orientation.w,
    );
    let qb = glam::Quat::from_xyzw(
        b.orientation.x,
        b.orientation.y,
        b.orientation.z,
        b.orientation.w,
    );
    let mid = qa.slerp(qb, 0.5);
    xr::Posef {
        orientation: xr::Quaternionf {
            x: mid.x,
            y: mid.y,
            z: mid.z,
            w: mid.w,
        },
        position: xr::Vector3f {
            x: 0.5 * (a.position.x + b.position.x),
            y: 0.5 * (a.position.y + b.position.y),
            z: 0.5 * (a.position.z + b.position.z),
        },
    }
}

/// The gameplay-resume auto-recenter state (see [`auto_recenter_tick`]).
static AUTO_RECENTER: Mutex<AutoRecenterState> = Mutex::new(AutoRecenterState {
    armed: false,
    last_anchor: None,
    settled_frames: 0,
});

struct AutoRecenterState {
    /// Armed to fire one recenter once gameplay settles. Set while not in settled gameplay.
    armed: bool,
    /// The head anchor last frame, for the settle test.
    last_anchor: Option<glam::Vec3>,
    /// Consecutive frames the anchor has stayed within [`ANCHOR_SETTLE_EPS`].
    settled_frames: u32,
}

/// Frames the head anchor must hold within [`ANCHOR_SETTLE_EPS`] before an armed auto-recenter fires
/// -- long enough for the resume-from-menu entry animation (standing up from the car) to finish.
const ANCHOR_SETTLE_FRAMES: u32 = 24;
/// Per-frame head-anchor movement (metres) below which the pose counts as settled: above idle sway,
/// below any scripted or locomotion motion.
const ANCHOR_SETTLE_EPS: f32 = 0.01;
