//! The aim depth for the center (reticle) layer, recorded from the grapple reticle's
//! world-to-screen calls and smoothed with a critically-damped exponential.
//!
//! `CHUDUI::UpdateGrappleReticle` projects the grapple's world points through a default-VP
//! wrapper each frame; its world point is the surface the player is about to interact with --
//! the single most depth-meaningful point on the HUD. The hook records the point's distance from
//! the panel anchor here; the center layer reads the smoothed depth as its distance, so the
//! reticle group sits at the vergence of the thing it targets. When no aim point has been
//! recorded recently (no grapple reticle on screen), the depth eases back to the configured
//! center-layer distance.

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
    /// The most recently recorded raw depth.
    target: f32,
    /// When the raw depth was last recorded.
    recorded_at: Option<Instant>,
    /// When the smoothed value was last advanced.
    updated_at: Option<Instant>,
}

static STATE: Mutex<AimState> = Mutex::new(AimState {
    smoothed: 3.0,
    target: 3.0,
    recorded_at: None,
    updated_at: None,
});

/// Record the aim point's depth (game thread, from the grapple-reticle hook). `depth` is the
/// distance from the panel anchor to the aim world point, in meters.
pub fn record(depth: f32) {
    let mut state = STATE.lock();
    state.target = depth;
    state.recorded_at = Some(Instant::now());
}

/// The smoothed aim depth for the center layer (render thread, eye 0). Eases toward the last
/// recorded depth, or toward `base_distance` when the recording has gone stale.
pub fn current(base_distance: f32) -> f32 {
    let mut state = STATE.lock();
    let now = Instant::now();

    let stale = state
        .recorded_at
        .is_none_or(|t| t.elapsed().as_secs_f32() > STALE_AFTER_SECONDS);
    let target = if stale { base_distance } else { state.target };

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
