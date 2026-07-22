//! The single-pass stereo rewrite layer.
//!
//! Rewrites DXBC shader bytecode so one instance-doubled draw renders both eyes. Four functional
//! areas each target a different shader family: `remap` retargets an opaque model VS's per-eye `cb0`
//! rows to a mod-owned `cb13`, `reproject` post-multiplies a baked-WVP VS's clip position by a
//! per-eye `M_eye`, and `terrain` handles the tessellation VS and DS ends of the terrain path. The
//! shared DXBC scaffolding lives in `common`.

mod common;
mod remap;
mod reproject;
mod terrain;

pub use common::{MEYE_ROW_BASE, STEREO_CB_REGISTER, STEREO_CB_ROWS, STEREO_REPROJ_CB_ROWS};
pub use remap::patch_vertex_shader;
pub use reproject::reproject_vertex_shader;
pub use terrain::{inject_eye_forward_vertex_shader, reproject_domain_shader};
