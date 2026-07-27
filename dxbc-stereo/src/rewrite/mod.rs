//! The single-pass stereo rewrite layer.
//!
//! Rewrites DXBC shader bytecode so one instance-doubled draw renders both eyes. Four functional
//! areas each target a different shader family: `remap` retargets an opaque model VS's per-eye `cb0`
//! rows to a mod-owned `cb13`, `reproject` post-multiplies a baked-WVP VS's clip position by a
//! per-eye `M_eye`, and `terrain` handles the tessellation VS and DS ends of the terrain path. The
//! shared DXBC scaffolding lives in `common`.
//!
//! `ssdecal` is the one pixel-shader rewrite: it separates the screen-space decal permutations'
//! depth-texture UV from the viewport-normalized UV their reconstruction basis needs, so the two can
//! address differently-sized spaces.

mod common;
mod remap;
mod reproject;
mod ssdecal;
mod terrain;

pub use common::{MEYE_ROW_BASE, STEREO_CB_REGISTER, STEREO_CB_ROWS, STEREO_REPROJ_CB_ROWS};
pub use remap::patch_vertex_shader;
pub use reproject::reproject_vertex_shader;
pub use ssdecal::{SSDECAL_EYE_BIAS_REGISTER, bias_ssdecal_depth_uv};
pub use terrain::{
    forward_eye_hull_shader, inject_eye_forward_vertex_shader, reproject_domain_shader,
};
