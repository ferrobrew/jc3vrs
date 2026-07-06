//! The aim depth for the center (reticle) layer, smoothed with a critically-damped exponential.
//!
//! The source is the game's own aim system: `CHUDUI::UpdateGrappleReticle` projects the smoothed
//! aim position (`CPlayerAimControl`'s `m_AimPos`, lerped) as the *first* world-to-screen call of
//! its frame, every frame the HUD reticle updates -- the base grapple reticle is the general aim
//! cursor, present whether or not the grapple is in use. Only that first call is recorded; the
//! function's later calls (the wire-attachment point and the grip-radius sample) are different
//! points entirely. When no aim point has been recorded recently, the depth eases back to the
//! configured center-layer distance.

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
    /// The most recently recorded aim depth.
    target: f32,
    /// When the aim depth was last recorded.
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

/// Record the aim point's depth (game thread, from the grapple-reticle hook's first call of the
/// frame). `depth` is the distance from the panel anchor to the aim world point, in meters.
pub fn record(depth: f32) {
    let mut state = STATE.lock();
    state.target = depth;
    state.recorded_at = Some(Instant::now());
}

/// The smoothed aim depth for the center layer (render thread, eye 0). Eases toward the last
/// recorded aim depth, or toward `base_distance` when the recording has gone stale.
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
