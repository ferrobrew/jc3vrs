//! FSR configuration. See `docs/mod/rendering/fsr.md`.

use serde::{Deserialize, Serialize};

/// FSR anti-aliasing / upscaling settings. When `enabled`, FSR runs in place of the engine's SMAA
/// (which is suppressed); off restores the engine AA. See `docs/mod/rendering/fsr.md`.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct FsrConfig {
    /// Master switch: run FSR and suppress the engine AA. Off = engine SMAA as normal, FSR idle.
    pub enabled: bool,
    /// Apply the temporal sub-pixel jitter (camera projection + dispatch). FSR needs it to
    /// reconstruct sub-pixel detail, but it also excites a blob-scale shadow-term flicker whose
    /// mechanism resisted a long bisection (issue #10) -- every identified jitter coupling was
    /// fixed or ruled out (motion vectors, the post-chain double-run, the LOD dissolve, the shadow
    /// fit) and the flicker still tracked the jitter, so it ships off: stability over sharpness.
    /// Enable to trade back.
    pub jitter: bool,
    /// The sign convention of the *camera-side* jitter (the clip-space translation on the
    /// projection); the dispatch side always reports FSR's canonical offset. The two sides must
    /// agree or FSR de-jitters in the wrong direction and high-contrast detail pulses at the Halton
    /// cadence (the localised one-frame flicker of issue #10) -- a runtime knob so the convention can
    /// be settled live, like [`mv_sign`](Self::mv_sign). Default `(1, 1)` (the FSR-documented
    /// `(2*jx/w, -2*jy/h)` mapping).
    pub jitter_sign: (f32, f32),
    /// Scale on the jitter amplitude (0..1), applied consistently to the camera and the dispatch. A
    /// diagnostic lever: if no [`jitter_sign`](Self::jitter_sign) fixes the pulse but halving the
    /// amplitude softens it, the cause is FSR's own lock dynamics rather than a convention mismatch.
    pub jitter_scale: f32,
    /// Optional RCAS sharpening strength (0..1); `None` disables the sharpening pass.
    pub sharpness: Option<f32>,
    /// Feed motion vectors to FSR. Off makes FSR reproject with zero motion (ghosts moving objects) --
    /// a debug A/B to confirm the decode is helping.
    pub motion_vectors: bool,
    /// The sign/axis convention applied to the decoded UV motion before FSR. The decode math is now
    /// RE-exact (see `docs/mod/rendering/fsr.md`); only FSR's expected sign/Y direction is empirical -- a wrong sign
    /// is visually obvious (trails point backwards). Defaults to `(1, -1)` (UV is Y-down; FSR's
    /// convention TBD against on-screen motion).
    pub mv_sign: (f32, f32),
    /// Correct the motion vectors for stereo in the decode pass. The engine's velocity encodes
    /// `curUV - prevUV` with the *per-eye* current view-projection but the single sim-side *center*
    /// previous view-projection, so every static pixel carries a spurious depth-dependent parallax
    /// vector of opposite sign per eye, and FSR mis-reprojects each eye's temporal history -- the
    /// per-eye shadow-edge flicker under head motion (issue #10). The correction re-anchors each
    /// vector at the eye's own previous pose ([`crate::stereo::VpHistory`]); a no-op without stereo
    /// disparity.
    pub mv_stereo_correction: bool,
    /// Cancel the camera jitter from the motion vectors in the decode pass. The engine measures
    /// `curUV` under the jittered projection, so every stored vector carries the frame's sub-pixel
    /// jitter as a constant offset, while FSR expects jitter-free motion. A correctness fix for
    /// whenever [`jitter`](Self::jitter) is on (it was not the issue-10 flicker); a no-op while
    /// jitter is off.
    pub mv_jitter_cancel: bool,
}
impl FsrConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            jitter: false,
            jitter_sign: (1.0, 1.0),
            jitter_scale: 1.0,
            sharpness: Some(0.2),
            motion_vectors: true,
            mv_sign: (1.0, -1.0),
            mv_stereo_correction: true,
            mv_jitter_cancel: true,
        }
    }
}
