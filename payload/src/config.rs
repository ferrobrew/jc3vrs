//! Runtime configuration: every user-facing toggle, consolidated into one mutex-guarded struct with
//! sub-structs by concern. The debug UI reads/writes the whole struct; hooks copy out the field(s)
//! they need at the top of a detour. Live engine-interface state (the current eye, frame counters,
//! the trace arm-flag) does NOT live here -- see [`crate::stereo::StereoState`] and the per-subsystem
//! runtime statics.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::hud::HudConfig;

/// The global runtime configuration. Cheap to lock (uncontended `parking_lot::Mutex`); read it at the
/// top of a hook and release before doing engine work.
pub static CONFIG: Mutex<Config> = Mutex::new(Config::new());

/// Snapshot the whole config (for the trace manifest / bulk UI reads).
pub fn get() -> Config {
    CONFIG.lock().clone()
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub stereo: StereoConfig,
    pub exposure: ExposureConfig,
    pub post_fx: PostFxConfig,
    pub camera: CameraConfig,
    pub fsr: FsrConfig,
    pub hud: HudConfig,
}
impl Config {
    pub const fn new() -> Self {
        Self {
            stereo: StereoConfig::new(),
            exposure: ExposureConfig::new(),
            post_fx: PostFxConfig::new(),
            camera: CameraConfig::new(),
            fsr: FsrConfig::new(),
            hud: HudConfig::new(),
        }
    }

    /// Lock the global config, run `f` against it, and return the result -- the terse read path for
    /// hooks: `Config::lock_query(|c| c.post_fx.skip_sun_halo)`. The lock is held only for `f`.
    pub fn lock_query<R>(f: impl FnOnce(&Config) -> R) -> R {
        f(&CONFIG.lock())
    }
}

/// Stereo rendering toggles. The live per-eye runtime state is [`crate::stereo::StereoState`].
#[derive(Clone, Serialize, Deserialize)]
pub struct StereoConfig {
    /// Master switch: render the scene twice, once per eye.
    pub enabled: bool,
    /// Apply the per-eye IPD camera offset.
    pub cameras: bool,
    /// Interpupillary distance, in metres.
    pub ipd: f32,
    /// Force SMAA 1x in stereo (T2X's shared history ghosts across the two eye dispatches).
    pub force_smaa_1x: bool,
    /// Force the SSAO pass into its "first pass" state before each stereo eye, so each eye computes AO
    /// fresh from its own depth instead of blending against the other eye's history. The SSAO history
    /// index advances once per dispatch (inside CRenderBlockSSAO::Draw), so without this a stereo
    /// render double-steps it and the two eyes compound. Kept on by default.
    pub force_ssao_first_pass: bool,
    /// Which eye reaches the screen (debug A/B).
    pub present_eye_0: bool,
    /// Restore the TAA-jitter / shadow-phase counters between eyes.
    pub restore_frame_counters: bool,
    /// Skip SetupRenderFrameData on eye 1 (experimental; normally inert).
    pub gate_setup_render_frame_data: bool,
    /// Skip HandBackBuffers on eye 1.
    pub gate_hand_back_buffers: bool,
    /// Zero the post-effect dt on eye 1 (so once-per-frame accumulators do not double-step).
    pub gate_eye1_dt: bool,
}
impl StereoConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            cameras: true,
            ipd: 0.068,
            force_smaa_1x: true,
            force_ssao_first_pass: true,
            present_eye_0: false,
            restore_frame_counters: true,
            gate_setup_render_frame_data: false,
            gate_hand_back_buffers: false,
            gate_eye1_dt: true,
        }
    }
}

/// Auto-exposure toggles.
#[derive(Clone, Serialize, Deserialize)]
pub struct ExposureConfig {
    /// Skip the per-frame auto-exposure metering on eye 1 (the stereo-darkening fix).
    pub gate: bool,
    /// Pin `m_CurrentExposure` to `forced_value` instead of the engine's auto-exposure (A/B aid).
    pub force: bool,
    /// The pinned exposure value, used when `force` is set.
    pub forced_value: f32,
}
impl ExposureConfig {
    pub const fn new() -> Self {
        Self {
            gate: true,
            force: false,
            forced_value: 0.11,
        }
    }
}

/// Post-effect skip toggles (bisection aids / VR cleanups).
#[derive(Clone, Serialize, Deserialize)]
pub struct PostFxConfig {
    pub skip_motion_blur: bool,
    pub skip_motion_blur_recon: bool,
    pub skip_dof: bool,
    pub dof_no_reproject: bool,
    pub skip_fade: bool,
    pub skip_glare: bool,
    pub skip_player_damage: bool,
    pub skip_sun_halo: bool,
    pub skip_histogram: bool,
}
impl PostFxConfig {
    pub const fn new() -> Self {
        Self {
            skip_motion_blur: false,
            skip_motion_blur_recon: false,
            skip_dof: false,
            dof_no_reproject: true,
            skip_fade: false,
            skip_glare: false,
            skip_player_damage: false,
            skip_sun_halo: false,
            skip_histogram: false,
        }
    }
}

/// VR head/body camera settings (was `hooks::camera::CameraSettings`).
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct CameraConfig {
    pub enabled: bool,
    pub body_offset: glam::Vec3,
    pub head_offset: glam::Vec3,
    pub use_eye_matrices: bool,
    pub blurs_enabled: bool,
    pub always_use_t1: bool,
}
impl CameraConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            body_offset: glam::Vec3::new(0.0, 0.1, 0.0),
            head_offset: glam::Vec3::new(0.0, -0.1, 0.0),
            use_eye_matrices: true,
            blurs_enabled: false,
            always_use_t1: false,
        }
    }
}

/// FSR anti-aliasing / upscaling settings. When `enabled`, FSR runs in place of the engine's SMAA
/// (which is suppressed); off restores the engine AA. See `docs/fsr.md`.
#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct FsrConfig {
    /// Master switch: run FSR and suppress the engine AA. Off = engine SMAA as normal, FSR idle.
    pub enabled: bool,
    /// Apply the temporal sub-pixel jitter (camera projection + dispatch). FSR needs this to
    /// reconstruct detail; without it FSR just blurs. A debug toggle to confirm the jitter's effect.
    pub jitter: bool,
    /// Optional RCAS sharpening strength (0..1); `None` disables the sharpening pass.
    pub sharpness: Option<f32>,
    /// Feed motion vectors to FSR. Off makes FSR reproject with zero motion (ghosts moving objects) --
    /// a debug A/B to confirm the decode is helping.
    pub motion_vectors: bool,
    /// The sign/axis convention applied to the decoded UV motion before FSR. The decode math is now
    /// RE-exact (see `docs/fsr.md`); only FSR's expected sign/Y direction is empirical -- a wrong sign
    /// is visually obvious (trails point backwards). Defaults to `(1, -1)` (UV is Y-down; FSR's
    /// convention TBD against on-screen motion).
    pub mv_sign: (f32, f32),
}
impl FsrConfig {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            jitter: true,
            sharpness: Some(0.2),
            motion_vectors: true,
            mv_sign: (1.0, -1.0),
        }
    }
}
