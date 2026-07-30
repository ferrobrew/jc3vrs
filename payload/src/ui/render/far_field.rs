//! The near/far draw-list split (#32): its levers and the per-pass split counters.

use crate::{
    config,
    far_field::FarFieldMode,
    ui::render::{passes::render_pass_name, single_pass::collapse_configured},
};

pub(super) fn section(ui: &mut egui::Ui, cfg: &mut config::Config) {
    ui.collapsing("Far field (#32, experimental)", |ui| {
        ui.checkbox(
            &mut cfg.far_field.enabled,
            "Enable near/far draw-list split (engine depth buckets)",
        )
        .on_hover_text(
            "Registers a depth-bucket boundary on the scene passes so their once-per-frame sort \
             produces a contiguous [near][far] list; the modes below window the draw onto one \
             run. Off restores the stock single bucket.",
        );
        ui.add(
            egui::Slider::new(&mut cfg.far_field.threshold_m, 50.0..=2000.0)
                .logarithmic(true)
                .text("Threshold (m)"),
        )
        .on_hover_text(
            "Instance-centre distance; large objects whose centre is past the threshold but \
             whose extent reaches nearer classify as far, so keep it conservative",
        );
        ui.horizontal(|ui| {
            ui.label("Far-regime types:");
            ui.text_edit_singleline(&mut cfg.far_field.gated_types)
                .on_hover_text(
                    "Registry type names (comma-separated) whose draws are inherently distant \
                     and skip with the far field, with no distance split — find candidates by \
                     bisecting the Diagnostics tab's render-block-type registry",
                );
        });
        let gated = crate::far_field::gated_type_names();
        if !gated.is_empty() {
            ui.label(format!("gated: {}", gated.join(", ")));
        }
        // Keep the IsEnabled overrides in sync with the list (and drop them all when the split is
        // disabled); `sync_type_gates` no-ops when nothing changed.
        crate::far_field::sync_type_gates(if cfg.far_field.enabled {
            &cfg.far_field.gated_types
        } else {
            ""
        });
        ui.horizontal(|ui| {
            ui.label("Mode:");
            for (mode, label, hover) in [
                (
                    FarFieldMode::Collect,
                    "Collect",
                    "Split + counters only; draw everything",
                ),
                (
                    FarFieldMode::SkipFar,
                    "Skip far",
                    "Far field vanishes on both eyes",
                ),
                (
                    FarFieldMode::SkipNear,
                    "Skip near",
                    "Far field in isolation",
                ),
                (
                    FarFieldMode::SkipFarEye1,
                    "Skip far, eye 1",
                    "The sharing candidate: eye 1 skips the far run",
                ),
                (
                    FarFieldMode::Share,
                    "Share",
                    "Render the far field once per frame (a third, far-only dispatch) and \
                     composite its G-buffer under both eyes; requires stereo",
                ),
            ] {
                // `Share` needs the two per-eye dispatches it composites into, which the single-pass
                // collapse does away with; the two are mutually exclusive.
                let blocked = mode == FarFieldMode::Share && collapse_configured(cfg);
                ui.add_enabled_ui(!blocked, |ui| {
                    ui.selectable_value(&mut cfg.far_field.mode, mode, label)
                        .on_hover_text(if blocked {
                            "Unavailable while the single-pass collapse is on: the collapse renders \
                             both eyes in one walk, so there are no per-eye dispatches to share a \
                             far G-buffer between"
                        } else {
                            hover
                        });
                });
            }
        });
        if ui
            .button("Dump split state (log)")
            .on_hover_text(
                "Log the next frame's per-pass classification state — buckets, keys, and the \
                 terrain blocks' placement fields — at INFO under the far_field target",
            )
            .clicked()
        {
            crate::far_field::request_dump(16);
        }
        stats_table(ui);
    });
}

/// The per-pass near/far split counters, freshest first. Entries older than a second (passes that
/// stopped drawing, or the split disabled) are dropped from view.
fn stats_table(ui: &mut egui::Ui) {
    let mut stats = crate::far_field::stats_snapshot();
    stats.retain(|(_, s)| s.updated.elapsed().as_secs_f32() < 1.0);
    if stats.is_empty() {
        ui.label("(no split passes drawn in the last second)");
        return;
    }
    let (near, far) = stats.iter().fold((0u64, 0u64), |(n, f), (_, s)| {
        (n + u64::from(s.near), f + u64::from(s.far))
    });
    ui.label(format!(
        "Total: {near} near / {far} far ({:.0}% far)",
        100.0 * far as f64 / (near + far).max(1) as f64
    ));
    egui::Grid::new("far_field_stats")
        .striped(true)
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Pass").strong());
            ui.label(egui::RichText::new("Near").strong());
            ui.label(egui::RichText::new("Far").strong());
            ui.label(egui::RichText::new("Windowed").strong());
            ui.end_row();
            for (id, s) in stats {
                ui.label(format!("{:#04X} {}", id, render_pass_name(id)));
                ui.label(s.near.to_string());
                ui.label(s.far.to_string());
                ui.label(if s.windowed { "yes" } else { "" });
                ui.end_row();
            }
        });
}
