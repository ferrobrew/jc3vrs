//! The Render tab: how the frame is produced — the core stereo widgets, FSR, and the stereo
//! correctness/quality levers — plus the render-thread capture state that feeds the Previews tab
//! and the VR blit.
//!
//! The tab's body is assembled from one section module per concern; the capture state lives in
//! [`capture`], independent of whether the tab is ever drawn.

mod capture;
mod corrections;
mod far_field;
mod passes;
mod post_fx;
mod resolution;
mod single_pass;
mod stereo;

use crate::config;
pub use crate::ui::render::capture::{
    EGUI_DEBUG_RENDER_STATE, EguiDebugRenderState, POST_STAGE_DOF, POST_STAGE_MB,
    capture_main_color, capture_post_stage, install, mark_previews_visible, previews_visible,
};

/// Debug-UI only: swap the two eyes in the side-by-side stereo preview, so the pair can be fused
/// cross-eyed (left image -> right eye) instead of parallel (left image -> left eye). Read by the
/// F10 capture composite so the recording window fuses the same way as the preview.
pub(crate) static STEREO_CROSS_EYED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

pub fn egui_debug_render(ui: &mut egui::Ui) {
    let mut cfg = config::CONFIG.lock();
    let cfg = &mut *cfg;

    stereo::section(ui, cfg);
    far_field::section(ui, cfg);
    corrections::section(ui, cfg);
    single_pass::section(ui, cfg);
    resolution::section(ui, cfg);
    post_fx::section(ui, cfg);
}
