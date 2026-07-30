//! The core stereo widgets: the double-Draw toggles, the per-eye camera offset, and FSR.

use crate::config;

pub(super) fn section(ui: &mut egui::Ui, cfg: &mut config::Config) {
    ui.checkbox(&mut cfg.stereo.enabled, "Stereo (double-Draw)")
        .on_hover_text("Issue a second game.Draw per frame; CClock::Update is gated to once/frame");

    ui.checkbox(
        &mut cfg.stereo.cameras,
        "Stereo cameras (per-eye IPD offset)",
    )
    .on_hover_text("Offset the active camera per eye so the two draws diverge");
    ui.add(egui::Slider::new(&mut cfg.stereo.ipd, 0.0..=100.0).text("IPD (m)"));

    ui.collapsing("FSR", |ui| {
        ui.checkbox(
            &mut cfg.fsr.enabled,
            "Anti-aliasing (replaces the engine SMAA)",
        );
        ui.checkbox(
            &mut cfg.fsr.jitter,
            "Temporal jitter (off = FSR blurs; A/B to confirm the jitter)",
        );
        ui.horizontal(|ui| {
            ui.label("Jitter sign (camera side):");
            let (sx, sy) = &mut cfg.fsr.jitter_sign;
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
        })
        .response
        .on_hover_text(
            "Must agree with the offset reported to the FSR dispatch, or fine detail \
                 pulses at the jitter cadence -- flip live to settle the convention",
        );
        ui.add(
            egui::Slider::new(&mut cfg.fsr.jitter_scale, 0.0..=1.0)
                .text("Jitter scale (diagnostic)"),
        );
        ui.horizontal(|ui| {
            let mut sharpen = cfg.fsr.sharpness.is_some();
            ui.checkbox(&mut sharpen, "Sharpening");
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
            "Motion vectors (off = ghosts moving objects; A/B the decode)",
        );
        ui.checkbox(
            &mut cfg.fsr.mv_jitter_cancel,
            "MV jitter cancel (vectors carry the camera jitter; FSR wants them jitter-free)",
        )
        .on_hover_text(
            "The +/-0.5 px jitter wobble in the vectors flips FSR's history validation over \
                 steep depth gradients -- region-scale one-frame pops at the jitter cadence",
        );
        ui.checkbox(
            &mut cfg.fsr.mv_stereo_correction,
            "Stereo MV correction (re-anchor velocity at the per-eye camera)",
        )
        .on_hover_text(
            "The engine's velocity reprojects with the center camera's previous \
                 view-projection; each eye rasterizes with its own, so static pixels carry a \
                 spurious parallax vector and FSR flickers shadow edges per eye under motion \
                 (issue #10)",
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
    });
}
