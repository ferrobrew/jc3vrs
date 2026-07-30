//! The post-effect levers: the reprojection passes, and the individual stages to skip when
//! bisecting an artifact to one of them.

use crate::config;

pub(super) fn section(ui: &mut egui::Ui, cfg: &mut config::Config) {
    ui.collapsing("Post-FX (reprojection passes, both eyes)", |ui| {
        ui.checkbox(
            &mut cfg.post_fx.skip_motion_blur,
            "Skip MotionBlur::Apply (whole pass)",
        );
        ui.checkbox(
            &mut cfg.post_fx.skip_motion_blur_recon,
            "Skip MotionBlur recon (if pass not skipped)",
        );
        ui.checkbox(
            &mut cfg.post_fx.dof_no_reproject,
            "DoF: plain composite, no reprojection (keeps picture)",
        );
        ui.checkbox(
            &mut cfg.post_fx.skip_dof,
            "Skip DepthOfField::Apply (washes out!)",
        );
    });

    ui.collapsing("Post-FX stages (skip to bisect)", |ui| {
        ui.checkbox(
            &mut cfg.post_fx.skip_histogram,
            "Exposure histogram (stalls auto-exposure)",
        );
        ui.checkbox(&mut cfg.post_fx.skip_glare, "Glare / bloom");
        ui.checkbox(&mut cfg.post_fx.skip_fade, "Fade");
        ui.checkbox(&mut cfg.post_fx.skip_sun_halo, "Sun halo");
        ui.checkbox(
            &mut cfg.post_fx.skip_player_damage,
            "Player-damage vignette",
        );
    });
}
