//! Per-marker depth recording for the marker-layer warp.
//!
//! The game's marker placement (`CUIManager::Get2DInfo`) hands the mod every world-anchored
//! marker's world position each frame; the hook records where each on-screen marker landed on the
//! panel texture and how far its world point is from the panel anchor. The marker-layer draw then
//! warps its grid mesh so each marker's neighborhood sits at that world depth (see
//! `shaders/hud_layer_vs.hlsl`).
//!
//! Recording happens on the game thread during the HUD update; the render thread takes the frame's
//! set at the eye-0 draw. A single pending buffer suffices: the game update for frame N completes
//! before frame N's draws consume it.

use parking_lot::Mutex;

/// One recorded on-screen marker: panel-texture UV plus its world depth from the panel anchor.
#[derive(Clone, Copy)]
pub struct MarkerDepth {
    pub u: f32,
    pub v: f32,
    /// Distance from the panel anchor to the marker's world point, in meters.
    pub depth: f32,
    /// The warp falloff radius around the marker, in texture-uv units.
    pub radius: f32,
}

/// The shader's marker capacity (matches `Markers[32]` in `hud_layer_vs.hlsl`).
pub const MARKER_CAPACITY: usize = 32;

/// Markers recorded since the last [`take_frame`], in call order. Capped at
/// [`MARKER_CAPACITY`]; the overflow is counted and logged once per session.
static PENDING: Mutex<Vec<MarkerDepth>> = Mutex::new(Vec::new());

/// Record an on-screen marker (from the `Get2DInfo` hook, game thread). Beyond the capacity the
/// marker is dropped -- the nearest markers matter most and near markers tend to be projected
/// first (the HUD updates the reticle and close POIs before far ones), but this is best-effort.
pub fn record(marker: MarkerDepth) {
    let mut pending = PENDING.lock();
    if pending.len() < MARKER_CAPACITY {
        pending.push(marker);
    } else {
        log_overflow_once();
    }
}

/// Take the frame's recorded markers (render thread, eye 0).
pub fn take_frame() -> Vec<MarkerDepth> {
    std::mem::take(&mut *PENDING.lock())
}

fn log_overflow_once() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        tracing::warn!(
            "hud markers: more than {MARKER_CAPACITY} on-screen markers in a frame; the excess \
             stays at the layer's base depth"
        );
    }
}
