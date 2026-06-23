//! The Debug tab: the rarely-used stereo/exposure/post-FX bisection toggles and the render-trace
//! dump. Only locks CONFIG.

use crate::{config, debug::trace};

pub fn egui_debug_debug(ui: &mut egui::Ui) {
    let mut cfg = config::CONFIG.lock();
    // Deferred so the trace button's start() doesn't re-lock CONFIG (it snapshots the config for the
    // manifest) while this guard is held -- parking_lot is not reentrant, so that self-deadlocks.
    let mut start_trace = false;

    ui.checkbox(
        &mut cfg.fsr.enabled,
        "FSR anti-aliasing (replaces the engine SMAA)",
    );
    ui.checkbox(
        &mut cfg.fsr.jitter,
        "FSR temporal jitter (off = FSR blurs; A/B to confirm the jitter)",
    );
    ui.horizontal(|ui| {
        let mut sharpen = cfg.fsr.sharpness.is_some();
        ui.checkbox(&mut sharpen, "FSR sharpening");
        match (sharpen, cfg.fsr.sharpness) {
            (true, None) => cfg.fsr.sharpness = Some(0.5),
            (false, Some(_)) => cfg.fsr.sharpness = None,
            _ => {}
        }
        if let Some(s) = cfg.fsr.sharpness.as_mut() {
            ui.add(egui::Slider::new(s, 0.0..=1.0).text("strength"));
        }
    });
    ui.checkbox(
        &mut cfg.fsr.motion_vectors,
        "FSR motion vectors (off = ghosts moving objects; A/B the decode)",
    );
    ui.horizontal(|ui| {
        ui.label("MV sign:");
        let (sx, sy) = &mut cfg.fsr.mv_sign;
        if ui.selectable_label(*sx > 0.0, "x+").clicked() {
            *sx = 1.0;
        }
        if ui.selectable_label(*sx < 0.0, "x-").clicked() {
            *sx = -1.0;
        }
        if ui.selectable_label(*sy > 0.0, "y+").clicked() {
            *sy = 1.0;
        }
        if ui.selectable_label(*sy < 0.0, "y-").clicked() {
            *sy = -1.0;
        }
    });
    ui.checkbox(
        &mut cfg.stereo.force_smaa_1x,
        "Force SMAA 1x in stereo (T2X ghosts across eyes)",
    );
    ui.checkbox(
        &mut cfg.stereo.force_ssao_first_pass,
        "Force SSAO first-pass per eye in stereo (stops cross-eye AO history blend)",
    );
    ui.checkbox(
        &mut cfg.stereo.restore_frame_counters,
        "Restore frame counters between eyes (fixes jitter/parity flicker)",
    );
    ui.checkbox(
        &mut cfg.stereo.present_eye_0,
        "Present eye 0 (else eye 1) -- flip to compare each eye live",
    );
    ui.horizontal(|ui| {
        if ui.button("Dump render trace (4 frames)").clicked() {
            start_trace = true;
        }
        let remaining = trace::active_frames();
        if remaining > 0 {
            ui.label(format!("tracing... {remaining} frames left"));
        } else if trace::TraceState::last_path().is_some() {
            ui.label("dumped");
        } else {
            ui.label("(writes next to the DLL)");
        }
    });

    ui.collapsing("Eye-1 gates (skip on second Draw)", |ui| {
        ui.checkbox(
            &mut cfg.stereo.gate_rotate_render_frame_data,
            "RotateRenderFrameData (RBI list flip -- the geometry fix)",
        );
        ui.checkbox(
            &mut cfg.exposure.gate,
            "Auto-exposure (SmoothedExposure + Histogram)",
        );
        ui.checkbox(
            &mut cfg.stereo.gate_eye1_dt,
            "Eye-1 dt=0 (world fade / sun / heat-haze step once per frame)",
        );
        ui.checkbox(
            &mut cfg.stereo.gate_setup_render_frame_data,
            "SetupRenderFrameData (per-batch list build, not the swap)",
        );
        ui.checkbox(
            &mut cfg.stereo.gate_hand_back_buffers,
            "HandBackBuffers (constant-buffer recycle)",
        );
    });

    ui.collapsing("Exposure A/B (pin m_CurrentExposure)", |ui| {
        ui.checkbox(
            &mut cfg.exposure.force,
            "Force exposure (pin after the engine's Update)",
        );
        ui.add(
            egui::Slider::new(&mut cfg.exposure.forced_value, 0.0..=0.5)
                .text("Forced exposure (~0.11 = non-stereo daylight)"),
        );
        ui.label(
            "A/B: enable in both stereo and non-stereo at the same value. Same brightness => the \
             darkening was the exposure loop; stereo still darker => a render path.",
        );
    });

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

    // Release CONFIG before start(), whose manifest snapshot re-locks it.
    drop(cfg);
    if start_trace {
        trace::TraceState::start(4);
    }
}
