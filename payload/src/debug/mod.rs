//! Debug subsystems: machinery (no egui) backing the debug UI. The render trace ([`trace`]) and the
//! per-eye render-camera snapshots ([`camera`]); their egui surfaces live under `crate::ui`.

pub mod camera;
pub mod coord_frame;
pub mod rt_hash;
pub mod stereo_diff;
pub mod trace;
