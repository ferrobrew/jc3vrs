//! The profiler's Performance-tab UI: a collapsible holding the enable toggle, the trace-capture
//! control, and — when scope collection is on — puffin's live flame graph.

use super::capture::{self, DEFAULT_CAPTURE_SECS};

/// Renders the profiler section, as a collapsible under the existing Performance readout.
pub fn egui_profiler(ui: &mut egui::Ui) {
    ui.collapsing("Profiler (issue #34)", |ui| {
        let mut enabled = super::ui_enabled();
        if ui
            .checkbox(&mut enabled, "Collect scopes (live flame graph)")
            .on_hover_text(
                "Enables per-frame CPU and GPU scope collection and the flame graph below. A trace \
                 capture turns this on for its duration regardless.",
            )
            .changed()
        {
            super::set_ui_enabled(enabled);
        }

        capture_controls(ui);

        if super::ui_enabled() {
            ui.separator();
            puffin_egui::profiler_ui(ui);
        } else if capture::is_recording() {
            ui.separator();
            ui.label("Capturing… (flame graph hidden; enable collection to watch live)");
        }
    });
}

/// The capture button, a live progress readout while recording, and the last dump's outcome.
fn capture_controls(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let recording = capture::is_recording();
        let button = egui::Button::new(if recording {
            "Capturing…".to_owned()
        } else {
            format!("Capture {DEFAULT_CAPTURE_SECS:.0} s trace")
        });
        if ui
            .add_enabled(!recording, button)
            .on_hover_text(
                "Records a few seconds of CPU and GPU frames and writes them next to the log as \
                 Chrome trace-event JSON (open in ui.perfetto.dev). Also bound to F9.",
            )
            .clicked()
        {
            capture::start(DEFAULT_CAPTURE_SECS);
        }

        if let Some((elapsed, total)) = capture::progress() {
            ui.add(
                egui::ProgressBar::new((elapsed / total).clamp(0.0, 1.0))
                    .text(format!("{elapsed:.1} / {total:.1} s")),
            );
        } else if capture::is_writing() {
            ui.spinner();
            ui.label("Writing trace…");
        }
    });

    match capture::last_result() {
        Some(Ok(path)) => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            ui.label(format!("Last capture: {name}"));
        }
        Some(Err(e)) => {
            ui.colored_label(
                egui::Color32::from_rgb(0xE0, 0x4C, 0x3C),
                format!("Capture failed: {e}"),
            );
        }
        None => {}
    }
}
