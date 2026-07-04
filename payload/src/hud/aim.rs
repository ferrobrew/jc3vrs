//! The aim depth for the center (reticle) layer, smoothed with a critically-damped exponential.
//!
//! Two sources, in priority order:
//!
//! 1. The grapple reticle's world point (`CHUDUI::UpdateGrappleReticle`'s world-to-screen calls):
//!    the exact surface the player is about to interact with, available while grappling.
//! 2. A world-anchored marker at the reticle (the auto-aim target POI lands exactly there): the
//!    exact target the game itself considers aimed-at, available while something is locked.
//! 3. The scene depth under the crosshair (the [`super::depth_probe`] readback): what the player
//!    is looking at or shooting, available always -- so the crosshair itself is a distance cue.
//!
//! When neither is fresh, the depth eases back to the configured center-layer distance.

use std::time::Instant;

use parking_lot::Mutex;

/// How long a recorded aim depth stays authoritative. Beyond this, the smoothed depth eases back
/// to the configured base distance (the reticle is not targeting anything).
const STALE_AFTER_SECONDS: f32 = 0.5;

/// The smoothing halflife, in seconds: long enough to swallow surface noise as the aim sweeps
/// across geometry, short enough to track a real target change quickly.
const HALFLIFE_SECONDS: f32 = 0.15;

/// The damped aim-depth state.
struct AimState {
    /// The smoothed depth, in meters.
    smoothed: f32,
    /// The most recently recorded grapple depth.
    grapple: f32,
    /// When the grapple depth was last recorded.
    grapple_at: Option<Instant>,
    /// The most recently recorded scene-probe depth.
    probe: f32,
    /// When the probe depth was last recorded.
    probe_at: Option<Instant>,
    /// The most recently recorded center-marker (auto-aim target) depth.
    center_marker: f32,
    /// When the center-marker depth was last recorded.
    center_marker_at: Option<Instant>,
    /// When the smoothed value was last advanced.
    updated_at: Option<Instant>,
}

static STATE: Mutex<AimState> = Mutex::new(AimState {
    smoothed: 3.0,
    grapple: 3.0,
    grapple_at: None,
    probe: 3.0,
    probe_at: None,
    center_marker: 3.0,
    center_marker_at: None,
    updated_at: None,
});

/// Record the grapple aim point's depth (game thread, from the grapple-reticle hook). `depth` is
/// the distance from the panel anchor to the aim world point, in meters.
pub fn record(depth: f32) {
    let mut state = STATE.lock();
    state.grapple = depth;
    state.grapple_at = Some(Instant::now());
}

/// Record the scene depth under the crosshair (render thread, from the depth probe), in meters.
pub fn record_probe(depth: f32) {
    let mut state = STATE.lock();
    state.probe = depth;
    state.probe_at = Some(Instant::now());
}

/// Record an aimed-at marker's depth (render thread; the recorded marker nearest the reticle
/// center, when one sits within the lock-on radius), in meters.
pub fn record_center_marker(depth: f32) {
    let mut state = STATE.lock();
    state.center_marker = depth;
    state.center_marker_at = Some(Instant::now());
}

/// The smoothed aim depth for the center layer (render thread, eye 0). Eases toward the freshest
/// source -- the grapple point over the scene probe -- or toward `base_distance` when both are
/// stale.
pub fn current(base_distance: f32) -> f32 {
    let mut state = STATE.lock();
    let now = Instant::now();

    let fresh =
        |t: Option<Instant>| t.is_some_and(|t| t.elapsed().as_secs_f32() <= STALE_AFTER_SECONDS);
    let target = if fresh(state.grapple_at) {
        state.grapple
    } else if fresh(state.center_marker_at) {
        state.center_marker
    } else if fresh(state.probe_at) {
        state.probe
    } else {
        base_distance
    };

    let dt = state
        .updated_at
        .map(|t| t.elapsed().as_secs_f32())
        .unwrap_or(0.016)
        .min(0.1);
    state.updated_at = Some(now);

    // Holden's critically-damped exponential, like the panel follow.
    let alpha = (1.0 - 2.0_f32.powf(-dt / HALFLIFE_SECONDS)).min(1.0);
    state.smoothed += (target - state.smoothed) * alpha;
    state.smoothed
}
