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

        let mut per_draw = super::per_draw_scopes();
        if ui
            .checkbox(&mut per_draw, "Per-draw render-block scopes")
            .on_hover_text(
                "Adds one puffin scope per render-block-type run (hundreds per frame) on the \
                 draw-submission path -- which inflates the very path it measures, so a capture \
                 taken with this on overstates submission cost. Off by default; the per-type block \
                 counts below are collected either way.",
            )
            .changed()
        {
            super::set_per_draw_scopes(per_draw);
        }

        let mut pass_timestamps = super::gpu::pass_timestamps_enabled();
        if ui
            .checkbox(&mut pass_timestamps, "GPU per-pass timestamps")
            .on_hover_text(
                "Brackets every render pass with a GPU timestamp pair, so a dispatch's GPU span \
                 splits into work and starvation instead of reading as one opaque block. On by \
                 default; turning it off leaves only the coarse per-seam brackets, and the busy \
                 figure degenerates to the whole span.",
            )
            .changed()
        {
            super::gpu::set_pass_timestamps_enabled(pass_timestamps);
        }

        capture_controls(ui);
        gpu_summary(ui);
        block_counts(ui);

        if super::ui_enabled() {
            ui.separator();
            puffin_egui::profiler_ui(ui);
        } else if capture::is_recording() {
            ui.separator();
            ui.label("Capturing… (flame graph hidden; enable collection to watch live)");
        }
    });
}

/// The last completed GPU summary window: the busy/starved decomposition and the CPU submit span
/// it has to be read against. Mirrors the periodic log line, for reading in-headset.
fn gpu_summary(ui: &mut egui::Ui) {
    let Some(summary) = super::gpu::summary() else {
        return;
    };
    ui.separator();
    ui.label(format!(
        "GPU/frame over {} frames ({:.1} dispatches): busy \u{2264} {:.2} ms, starved \u{2265} \
         {:.2} ms, idle between {:.2} ms",
        summary.frames,
        summary.dispatches_per_frame,
        summary.busy_ms,
        summary.starved_ms,
        summary.idle_ms,
    ))
    .on_hover_text(
        "Busy sums the per-pass GPU intervals and starved is the time between them, so busy is an \
         upper bound on real GPU work and starved a lower bound on real idle: starvation between \
         individual draws inside one pass is counted as busy. Read them against the CPU submit \
         span -- a GPU span that tracks submit is a fed-just-in-time pipeline, not shading cost.",
    );
    ui.label(format!("CPU submit/frame: {:.2} ms", summary.submit_ms));
}

/// The busiest render-block types of the last summary window, by blocks drawn.
fn block_counts(ui: &mut egui::Ui) {
    let counts = super::blocks::snapshot();
    if counts.is_empty() {
        return;
    }
    ui.collapsing("Render blocks drawn (current window)", |ui| {
        for count in counts.iter().take(16) {
            ui.label(format!(
                "{}: {} blocks in {} runs",
                count.name, count.blocks, count.runs
            ));
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
