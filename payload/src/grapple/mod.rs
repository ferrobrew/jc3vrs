//! Grapple comfort: keeps the grapple from throwing the VR view around (issue #36).
//!
//! The player aims the grapple by looking at the target, so the head pose already carries the
//! rotation toward it. The engine then rotates the character's root toward that same target —
//! first the fire act's directional alignment, then `NReeledInController::RotateToGrappleTarget`
//! during the reel — and the headpose composes the head onto the body's full world rotation, so
//! the target rotation is applied twice and the view swings violently past whatever was being
//! looked at. In VR that is a rapid rotation the inner ear never felt: a strong vestibular
//! conflict in the game's most-used traversal mechanic.
//!
//! This module filters the *body-driven* inputs to the headpose composition while the grapple owns
//! the character. The HMD's own tracking passes through untouched, in both rotation and position.
//! Three cooperating pieces:
//!
//! - **The body-frame hold** ([`filter_body_rotation`]): while the grapple is active, the body
//!   rotation the head composes onto is blended toward a filtered frame per
//!   [`GrappleComfortMode`] — by default the frame the previous render composed with, held from
//!   fire to landing so the view stays world-stable. The hold engages the moment a fire is
//!   committed (the fire act whips the body toward the target before the wire even spawns) and
//!   releases by blending the accumulated rotation back over `release_s`.
//! - **The yaw handoff**: an on-foot landing would otherwise return 10–25° of accumulated yaw to
//!   the view through the release blend. Instead the held heading is posted for the VR body-turn
//!   accumulator ([`take_body_yaw_retarget`]), the character turns to face where the player is
//!   looking via the game's own rate-limited turn machinery, and the hold stays engaged until the
//!   heading converges — the view never sweeps through the yaw.
//! - **The landing-snap absorber** ([`filter_anchor`]): the attach/landing animation teleports the
//!   head anchor up to a metre in one sim tick. Single-step velocity spikes beyond the configured
//!   threshold are absorbed into an offset that decays out of the view; sustained motion of any
//!   speed passes through 1:1, and the filtered anchor is always `raw + offset`, so the engine's
//!   tick interpolation is undisturbed.
//!
//! The filter advances on the engine's input tick and again on every rendered VR frame
//! ([`advance`]): the body starts rotating the instant the grapple acts, and a tick-only (~33 Hz)
//! state read leaves up to a tick of that rotation in the view before the filter can engage.
//!
//! The filtered rotation's yaw does not track the real body while the hold is engaged, so
//! consumers that steer the actual character (the VR body-turn accumulator) read the raw rotation
//! ([`crate::headpose::xr::body_rotation_raw`]), never the filtered one. Lock order across the
//! headpose is SIM → BODY_YAW → FILTER: this module's lock is innermost, so nothing under it may
//! call back into a locking `headpose` accessor.
//!
//! [`telemetry`] captures the filter's inputs and outputs to a CSV for offline analysis; every
//! behaviour above was diagnosed and verified through it.

use std::{
    sync::atomic::{AtomicU32, Ordering},
    time::Instant,
};

use glam::{Quat, Vec3};
use jc3gi::{character::character::Character, equipment::grappling_hook::GrapplingHookState};
use parking_lot::Mutex;

pub mod config;
pub mod telemetry;

pub use config::{GrappleComfortConfig, GrappleComfortMode};

use crate::headpose;

/// Advance the filter one step: read the hook's phase, run the engage/hold/handoff/release state
/// machine, and update the anchor absorber. `body` and `anchor` are the raw body rotation and
/// head-bone anchor as of this advance; `on_foot` gates the yaw handoff.
///
/// Called from both cadences — the input tick ([`crate::headpose::sim::on_input_tick`], game
/// thread) and the VR frame loop (render thread) — under one mutex, with the step `dt` measured
/// internally. `on_foot` arrives as a parameter because the input-tick caller already holds the
/// sim lock, and reading `sim::mode()` here would deadlock (SIM → FILTER is the only legal lock
/// order).
pub fn advance(body: Quat, anchor: Vec3, on_foot: bool, config: &GrappleComfortConfig) {
    let mut s = FILTER.lock();
    let now = Instant::now();
    let dt = s
        .last_advance
        .map(|last| (now - last).as_secs_f32())
        .unwrap_or(0.0)
        .clamp(0.0, 0.1);
    s.last_advance = Some(now);

    let (reeling, engaged_other) = reel_flags();
    let active = config.mode != GrappleComfortMode::Off && (reeling || engaged_other);

    // Reel end, on foot: start the yaw handoff. The body-turn accumulator is aimed at the held
    // view heading, and the hold stays engaged while the game's own turn machinery brings the
    // body around; the release blend then only carries the small pitch/roll residual. Airborne
    // releases (jumping off mid-reel into a fall or parachute) skip it: the accumulator only
    // steers on foot, so the hold would pin the view against a flying world for the full timeout.
    if s.was_active
        && !active
        && on_foot
        && config.yaw_handoff
        && config.mode == GrappleComfortMode::HoldView
        && headpose::source() == headpose::Source::Vr
    {
        s.pending_retarget = Some((headpose::sim::body_yaw_of(s.held), now));
        s.handoff_deadline =
            Some(now + std::time::Duration::from_secs_f32(config.handoff_timeout_s.max(0.0)));
    }
    s.was_active = active;

    // The handoff ends when the body's heading converges on the held one, at the deadline (the
    // body can be blocked from turning), when the character leaves its feet, or when a new reel
    // starts.
    if let Some(deadline) = s.handoff_deadline {
        let dyaw = headpose::sim::wrap_angle(
            headpose::sim::body_yaw_of(body) - headpose::sim::body_yaw_of(s.held),
        )
        .abs();
        if active || !on_foot || now >= deadline || dyaw < HANDOFF_CONVERGED_RAD {
            s.handoff_deadline = None;
        }
    }
    let hold = active || s.handoff_deadline.is_some();

    // On the engage edge, hold the frame the view *last composed*: the previous advance's body
    // under the current blend. The previous advance's body, not this one's, because the body
    // snaps toward the fire direction in the same step the grapple state flips — the current body
    // already carries rotation that would jump the view at engage. Under the current blend, not
    // raw, because a re-grapple during the release tail must hold the partially blended frame the
    // player is already looking through. Seamless at any blend level.
    if hold && !s.was_hold {
        let pre_snap = s.prev_body.unwrap_or(body);
        s.held = filter_with(pre_snap, s.mode, s.held, s.factor);
    }
    s.was_hold = hold;
    s.prev_body = Some(body);
    s.mode = config.mode;

    let target = if hold { 1.0 } else { 0.0 };
    let tau = if target > s.factor {
        config.engage_s
    } else {
        config.release_s
    };
    let alpha = if tau > f32::EPSILON {
        1.0 - (-dt / tau).exp()
    } else {
        1.0
    };
    s.factor = s.factor + (target - s.factor) * alpha;
    // The release decay is exponential and never truly reaches zero; snap the tail off so the
    // filter (and the absorber's arming) fully disarms a couple of seconds after a release.
    if !hold && s.factor < FACTOR_DISARM {
        s.factor = 0.0;
    }
    FACTOR_BITS.store(s.factor.to_bits(), Ordering::Relaxed);

    advance_absorber(&mut s, anchor, reeling, now, dt, config);
}

/// Apply the body-frame hold to a body rotation. At blend `0` the rotation passes through
/// untouched; at `1` the head composes onto the fully filtered frame.
pub fn filter_body_rotation(body: Quat) -> Quat {
    let s = FILTER.lock();
    filter_with(body, s.mode, s.held, s.factor)
}

/// Apply the landing-snap absorber to the body-driven head anchor: the raw anchor plus the
/// decaying absorbed offset (zero outside a snap). Always `raw + offset`, so pose-pair deltas
/// built from it keep their exact tick spacing.
pub fn filter_anchor(anchor: Vec3) -> Vec3 {
    anchor + FILTER.lock().anchor_offset
}

/// The current blend factor in `[0, 1]`, for UI display.
pub fn blend_factor() -> f32 {
    f32::from_bits(FACTOR_BITS.load(Ordering::Relaxed))
}

/// Take the pending yaw-handoff heading (world radians about +Y), if one is still fresh. Consumed
/// by the VR body-yaw accumulator on its on-foot ticks: the accumulator adopts it as the turn
/// target, so the game's own turn machinery brings the body around to the held view heading. A
/// landing may be a few ticks away from the release that requested it (the landing transition has
/// to settle before the character counts as on foot), so the request waits; it expires after a
/// short window so one that is never consumed — a reel that ends into a vehicle — cannot steer a
/// much later on-foot tick.
pub fn take_body_yaw_retarget() -> Option<f32> {
    let mut s = FILTER.lock();
    let (yaw, at) = s.pending_retarget.take()?;
    (at.elapsed().as_secs_f32() <= RETARGET_FRESH_S).then_some(yaw)
}

/// The landing-snap absorber step (see the module docs). The detection runs on the anchor's own
/// timeline: the anchor is tick-sampled (constant across the frames between sim ticks), so
/// per-advance deltas read as a `0, 0, full-tick-step` staircase that a per-advance velocity model
/// misreads as perpetual excess at ordinary fast-movement speeds. Steps are detected by the anchor
/// actually changing, with the step `dt` measured between changes.
fn advance_absorber(
    s: &mut GrappleFilter,
    anchor: Vec3,
    reeling: bool,
    now: Instant,
    dt: f32,
    config: &GrappleComfortConfig,
) {
    let threshold = config.anchor_snap_threshold_mps;
    if let Some(prev) = s.prev_anchor {
        let delta = anchor - prev;
        if delta.length_squared() > 0.0 {
            let step_dt = s
                .last_anchor_step
                .map(|last| (now - last).as_secs_f32())
                .unwrap_or(0.0)
                .clamp(0.0, 0.1);
            s.last_anchor_step = Some(now);
            if delta.length() > ANCHOR_TELEPORT_M {
                // A genuine teleport (fast travel, respawn): pass through and reset.
                s.anchor_offset = Vec3::ZERO;
                s.anchor_velocity = Vec3::ZERO;
            } else if step_dt > 1e-4 {
                let excess = delta - s.anchor_velocity * step_dt;
                // Armed only from the attach/landing onward, never during the zip: the only
                // positional snap the grapple produces is at the landing, so the zip — including
                // its launch acceleration, which a fresh velocity estimate would misread as
                // excess — passes through untouched.
                let armed = s.factor > f32::EPSILON && !reeling && threshold > 0.0;
                if armed && excess.length() > threshold * step_dt {
                    s.anchor_offset -= excess;
                    // Hard-capped so a mis-estimate degrades into a bounded, decaying lag, never
                    // a runaway detach from the body.
                    s.anchor_offset = s.anchor_offset.clamp_length_max(ANCHOR_OFFSET_MAX_M);
                }
                // The velocity estimate tracks the raw motion unconditionally, so it locks onto
                // the zip within a few steps and only genuine one-step spikes read as excess.
                let alpha = 1.0 - (-step_dt / ANCHOR_VELOCITY_EMA_S).exp();
                s.anchor_velocity = s.anchor_velocity.lerp(delta / step_dt, alpha);
            }
        }
    }
    s.prev_anchor = Some(anchor);
    let ease = config.anchor_snap_ease_s;
    s.anchor_offset = if ease > f32::EPSILON {
        s.anchor_offset * (-dt / ease).exp()
    } else {
        Vec3::ZERO
    };
}

/// The heading convergence (radians) below which the post-reel yaw handoff completes.
const HANDOFF_CONVERGED_RAD: f32 = 3.0 * std::f32::consts::PI / 180.0;

/// How long (seconds) a pending yaw-handoff heading stays valid awaiting an on-foot tick.
const RETARGET_FRESH_S: f32 = 2.0;

/// The blend below which a releasing filter snaps to fully disarmed (its rotational effect is long
/// imperceptible there).
const FACTOR_DISARM: f32 = 1e-3;

/// Anchor steps beyond this (metres) are genuine teleports: passed through, never absorbed.
const ANCHOR_TELEPORT_M: f32 = 5.0;

/// The time constant (seconds) of the anchor velocity estimate the snap detection compares
/// against. Short, so the estimate locks onto a reel's launch acceleration within a few steps.
const ANCHOR_VELOCITY_EMA_S: f32 = 0.05;

/// The absorbed offset never exceeds this (metres).
const ANCHOR_OFFSET_MAX_M: f32 = 1.0;

/// The filter's state: written by [`advance`] on the game thread's input tick and the render
/// thread's frame loop, read per-eye on the render thread.
struct GrappleFilter {
    /// The smoothed blend factor in `[0, 1]`.
    factor: f32,
    /// The held body frame: captured on the engage edge, frozen through the hold, blended out on
    /// release.
    held: Quat,
    /// The mode as of the last advance, so [`filter_body_rotation`] needs no config access.
    mode: GrappleComfortMode,
    /// The previous advance's instant, for the blend step `dt` (advances arrive on two cadences).
    last_advance: Option<Instant>,
    /// Whether the previous advance saw the grapple active (release-edge detection for the
    /// handoff).
    was_active: bool,
    /// Whether the previous advance held (engage-edge detection for the held-frame capture; the
    /// hold outlasts `was_active` through the handoff).
    was_hold: bool,
    /// While `Some`, the post-reel yaw handoff is running: the hold stays engaged and the body
    /// turns toward the held heading.
    handoff_deadline: Option<Instant>,
    /// The heading the yaw handoff wants the body turned toward, awaiting consumption by the VR
    /// body-yaw accumulator (see [`take_body_yaw_retarget`]).
    pending_retarget: Option<(f32, Instant)>,
    /// The body rotation as of the previous advance — the frame the last render composed with —
    /// captured into the hold on the engage edge (the current advance's body already carries the
    /// grapple's fire-snap).
    prev_body: Option<Quat>,
    /// The anchor as of the previous advance, for the snap detection's step delta.
    prev_anchor: Option<Vec3>,
    /// When the anchor last changed: the anchor is tick-sampled, so the snap detection measures
    /// its steps against this rather than the advance cadence.
    last_anchor_step: Option<Instant>,
    /// The smoothed anchor velocity (m/s) the snap detection compares each step against.
    anchor_velocity: Vec3,
    /// The absorbed landing-snap offset, decaying toward zero (see [`filter_anchor`]).
    anchor_offset: Vec3,
}

static FILTER: Mutex<GrappleFilter> = Mutex::new(GrappleFilter {
    factor: 0.0,
    held: Quat::IDENTITY,
    mode: GrappleComfortMode::Off,
    last_advance: None,
    was_active: false,
    was_hold: false,
    handoff_deadline: None,
    pending_retarget: None,
    prev_body: None,
    prev_anchor: None,
    last_anchor_step: None,
    anchor_velocity: Vec3::ZERO,
    anchor_offset: Vec3::ZERO,
});

/// [`GrappleFilter::factor`]'s `f32` bits, mirrored for the lock-free UI readout.
static FACTOR_BITS: AtomicU32 = AtomicU32::new(0);

/// A snapshot of the filter's blend state, for [`telemetry`].
fn filter_snapshot() -> (GrappleComfortMode, f32, Quat) {
    let s = FILTER.lock();
    (s.mode, s.factor, s.held)
}

/// The pure filter: blend `body` toward the mode's filtered frame by `factor`.
fn filter_with(body: Quat, mode: GrappleComfortMode, held: Quat, factor: f32) -> Quat {
    if factor <= f32::EPSILON {
        return body;
    }
    let filtered = match mode {
        GrappleComfortMode::Off => return body,
        GrappleComfortMode::HoldView => held,
        GrappleComfortMode::LevelPitch => Quat::from_rotation_y(headpose::sim::body_yaw_of(body)),
    };
    body.slerp(filtered, factor.min(1.0)).normalize()
}

/// A snapshot of the local player's grappling-hook fields the filter reads each advance.
struct HookSnapshot {
    state: GrapplingHookState,
    /// Whether the active wire is live.
    wire: bool,
    /// Whether a fire is committed and awaiting the fire animation's release
    /// (`m_WaitingForGrappleFire`/`m_WaitingForTetherFire`).
    firing: bool,
}

/// Read the local player's grappling hook, or `None` when there is no local character or hook. A
/// plain field walk (character → inventory → hook), safe on both advance cadences.
fn hook_snapshot() -> Option<HookSnapshot> {
    unsafe {
        let character = Character::GetLocalPlayerCharacter();
        let character = character.as_ref()?;
        let hook = character.m_Inventory.m_GrapplingHook.as_ref()?;
        Some(HookSnapshot {
            state: hook.m_State,
            wire: hook.m_ActiveWire.exists(),
            firing: hook.m_WaitingForGrappleFire || hook.m_WaitingForTetherFire,
        })
    }
}

/// The local player's grapple phase as `(reeling, engaged_other)`: actively zipping
/// (`GHS_REELING_IN`), or otherwise in a phase the hold must cover:
///
/// - **Fire committed** (`m_WaitingFor*Fire`): the fire act's directional alignment whips the body
///   toward the target before the wire even spawns — measured up to 130° at ~1000°/s — and the
///   player aimed by looking there, so it is the same double-count as the reel itself.
/// - **Hook in flight** (`GHS_INACTIVE` with a live wire): bridges the gap between the fire act
///   and `GHS_REELING_IN`, so the hold never flickers between the two.
/// - **Reeled attachments**, gated on the active wire still existing: the hook's state records
///   the *last* reel outcome as much as the current one (leaving an attachment does not reliably
///   return it to `GHS_INACTIVE`; observed parked at `GHS_REELED_ATTACHED` for minutes of
///   ordinary play), while the wire is dropped when the attachment genuinely ends.
fn reel_flags() -> (bool, bool) {
    let Some(hook) = hook_snapshot() else {
        return (false, false);
    };
    match hook.state {
        GrapplingHookState::GHS_REELING_IN => (true, false),
        GrapplingHookState::GHS_REELED_ATTACHED
        | GrapplingHookState::GHS_REELED_HANG
        | GrapplingHookState::GHS_REELED_UPSIDEDOWN
        | GrapplingHookState::GHS_REELED_STUNT => (false, hook.wire || hook.firing),
        GrapplingHookState::GHS_INACTIVE => (false, hook.wire || hook.firing),
        _ => (false, hook.firing),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::EulerRot;

    /// At full blend, hold-view replaces the live body frame with the held one: the composed view
    /// stays wherever it was at reel start, no matter how the body rotates.
    #[test]
    fn hold_view_pins_to_held_frame() {
        let held = Quat::from_euler(EulerRot::YXZ, 0.3, 0.0, 0.0);
        let body = Quat::from_euler(EulerRot::YXZ, 1.2, 0.9, 0.0);
        let filtered = filter_with(body, GrappleComfortMode::HoldView, held, 1.0);
        assert!((filtered.dot(held).abs() - 1.0).abs() < 1e-6);
    }

    /// At full blend, level-pitch flattens a pitched body to its yaw alone.
    #[test]
    fn level_pitch_keeps_yaw_drops_pitch() {
        let body = Quat::from_euler(EulerRot::YXZ, 1.0, 0.8, 0.0);
        let filtered = filter_with(body, GrappleComfortMode::LevelPitch, Quat::IDENTITY, 1.0);
        let (yaw, pitch, roll) = filtered.to_euler(EulerRot::YXZ);
        assert!((yaw - 1.0).abs() < 1e-5);
        assert!(pitch.abs() < 1e-5);
        assert!(roll.abs() < 1e-5);
    }

    /// At zero blend the body rotation passes through untouched in every mode.
    #[test]
    fn zero_blend_is_identity() {
        let body = Quat::from_euler(EulerRot::YXZ, -0.4, 0.6, 0.1);
        for mode in [
            GrappleComfortMode::Off,
            GrappleComfortMode::HoldView,
            GrappleComfortMode::LevelPitch,
        ] {
            let filtered = filter_with(body, mode, Quat::IDENTITY, 0.0);
            assert!((filtered.dot(body).abs() - 1.0).abs() < 1e-6);
        }
    }
}
