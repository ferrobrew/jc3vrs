//! Runtime configuration: every user-facing toggle, consolidated into one mutex-guarded struct with
//! sub-structs by concern. The debug UI reads/writes the whole struct; hooks copy out the field(s)
//! they need at the top of a detour. Live engine-interface state (the current eye, frame counters,
//! the trace arm-flag) does NOT live here -- see [`crate::stereo::StereoState`] and the per-subsystem
//! runtime statics.
//!
//! These types derive `Serialize`/`Deserialize`, but only the serializing half is used -- the trace
//! manifest and the screenshot sidecar record the configuration a capture was taken under. Nothing
//! reads a configuration back, so the `#[serde(default)]` attributes scattered through the
//! sub-structs describe a load path that does not exist yet rather than one that is silently
//! consulting them. Defaults come from the `new()` constructors.
//!
//! The sub-structs are defined in their owning modules:
//! [`StereoConfig`](crate::stereo::config::StereoConfig) and
//! [`SinglePassConfig`](crate::stereo::config::SinglePassConfig) in `stereo::config`,
//! [`ExposureConfig`](crate::hooks::graphics_engine::config::ExposureConfig) and
//! [`PostFxConfig`](crate::hooks::graphics_engine::config::PostFxConfig) in
//! `hooks::graphics_engine::config`,
//! [`FarFieldConfig`](crate::far_field::FarFieldConfig) in `far_field`,
//! [`FoveationConfig`](crate::vr::FoveationConfig) in `vr::config`,
//! [`CameraConfig`](crate::hooks::camera::CameraConfig) in `hooks::camera::config`,
//! [`BodyIkConfig`](crate::hooks::character::BodyIkConfig) in `hooks::character::config`,
//! [`MovementConfig`](crate::hooks::input::MovementConfig) in `hooks::input::config`,
//! [`FsrConfig`](crate::fsr::FsrConfig) in `fsr::config`,
//! [`HudConfig`](crate::hud::HudConfig) in `hud::config`,
//! [`HeadPoseConfig`](crate::headpose::HeadPoseConfig) in `headpose::config`, and
//! [`VrConfig`](crate::vr::VrConfig) in `vr::config`.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::{
    far_field::FarFieldConfig,
    fsr::FsrConfig,
    headpose::HeadPoseConfig,
    hooks::{
        camera::CameraConfig,
        character::BodyIkConfig,
        graphics_engine::config::{ExposureConfig, PostFxConfig},
        input::MovementConfig,
    },
    hud::HudConfig,
    stereo::config::StereoConfig,
    vr::{FoveationConfig, VrConfig},
};

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
    #[serde(default)]
    pub foveation: FoveationConfig,
    #[serde(default)]
    pub far_field: FarFieldConfig,
    pub post_fx: PostFxConfig,
    pub camera: CameraConfig,
    pub movement: MovementConfig,
    pub fsr: FsrConfig,
    pub hud: HudConfig,
    pub headpose: HeadPoseConfig,
    pub body_ik: BodyIkConfig,
    pub vr: VrConfig,
}
impl Config {
    pub const fn new() -> Self {
        Self {
            stereo: StereoConfig::new(),
            exposure: ExposureConfig::new(),
            foveation: FoveationConfig::new(),
            far_field: FarFieldConfig::new(),
            post_fx: PostFxConfig::new(),
            camera: CameraConfig::new(),
            movement: MovementConfig::new(),
            fsr: FsrConfig::new(),
            hud: HudConfig::new(),
            headpose: HeadPoseConfig::new(),
            body_ik: BodyIkConfig::new(),
            vr: VrConfig::new(),
        }
    }

    /// Lock the global config, run `f` against it, and return the result -- the terse read path for
    /// hooks: `Config::lock_query(|c| c.post_fx.skip_sun_halo)`. The lock is held only for `f`.
    pub fn lock_query<R>(f: impl FnOnce(&Config) -> R) -> R {
        f(&CONFIG.lock())
    }
}
