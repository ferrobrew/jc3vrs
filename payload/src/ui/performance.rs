//! The Performance tab: the engine clock's frame-rate readouts and pacing controls, kept small so
//! it can be dragged out into its own window and watched while tuning render features.

use std::{collections::VecDeque, time::Instant};

use parking_lot::Mutex;

use super::util::patchbox;

pub fn egui_debug_performance(ui: &mut egui::Ui) {
    unsafe {
        let Some(clock) = jc3gi::clock::Clock::get() else {
            ui.label("(clock not reachable)");
            return;
        };

        ui.label(
            egui::RichText::new(format!("{:.1} FPS", clock.m_RealFPS))
                .size(28.0)
                .strong(),
        );
        ui.label(format!("{:.2} ms/frame (real)", clock.m_RealSPF * 1000.0));
        frame_time_chart(ui, clock.m_RealSPF * 1000.0);
        ui.separator();
        ui.label(format!("FPS: {}", clock.m_FPS));
        ui.label(format!("SPF: {}", clock.m_SPF));
        ui.label(format!("Real FPS: {}", clock.m_RealFPS));
        ui.label(format!("Real SPF: {}", clock.m_RealSPF));
        ui.label(format!("Update speed: {}", clock.m_UpdateSpeed));
        ui.label(format!("Force to FPS: {}", clock.m_ForceToThisFPS));
        ui.label(format!("Force to SPF: {}", clock.m_ForceToThisSPF));
        patchbox(ui, "Stop", &mut clock.m_Stop);
        patchbox(ui, "Force to FPS", &mut clock.m_ForceToFps);
    }

    far_field_summary(ui);

    // The profiler (issue #34): a collapsible with the live flame graph and trace capture, below
    // the frame-rate readout.
    #[cfg(feature = "profiler")]
    crate::profiler::ui::egui_profiler(ui);
}

/// The far-field split totals (issue #32), mirrored here so they can be watched alongside the
/// frame rate while tuning in the Render tab.
fn far_field_summary(ui: &mut egui::Ui) {
    let mut stats = crate::far_field::stats_snapshot();
    stats.retain(|(_, s)| s.updated.elapsed().as_secs_f32() < 1.0);
    if stats.is_empty() {
        return;
    }
    ui.separator();
    let (near, far, windowed) = stats.iter().fold((0u64, 0u64, false), |(n, f, w), (_, s)| {
        (n + u64::from(s.near), f + u64::from(s.far), w | s.windowed)
    });
    ui.label(format!(
        "Far field: {near} near / {far} far instances ({:.0}% far{})",
        100.0 * far as f64 / (near + far).max(1) as f64,
        if windowed { ", skipping active" } else { "" },
    ));
}

/// The rolling frame-time samples the chart draws: `(sample time, real ms/frame)`, capped to the
/// chart window. Sampled once per UI frame (the overlay runs once per real frame).
static FRAME_TIME_SAMPLES: Mutex<VecDeque<(Instant, f32)>> = Mutex::new(VecDeque::new());

/// Seconds of history the chart keeps and displays.
const CHART_WINDOW_S: f32 = 20.0;

/// A rolling frame-time chart, for judging the impact of render features (e.g. the far-field
/// share) where instantaneous numbers mislead — the VR runtime clamps the frame rate for
/// reprojection, so a cost change often shows as a *stability* change rather than a mean change.
fn frame_time_chart(ui: &mut egui::Ui, current_ms: f32) {
    let now = Instant::now();
    let mut samples = FRAME_TIME_SAMPLES.lock();
    samples.push_back((now, current_ms));
    while let Some(&(t, _)) = samples.front() {
        if now.duration_since(t).as_secs_f32() > CHART_WINDOW_S {
            samples.pop_front();
        } else {
            break;
        }
    }
    if samples.len() < 2 {
        return;
    }

    let max_ms = samples
        .iter()
        .map(|&(_, ms)| ms)
        .fold(f32::EPSILON, f32::max);
    let avg_ms = samples.iter().map(|&(_, ms)| ms).sum::<f32>() / samples.len() as f32;
    // Scale to the 99th percentile, snapped to sensible bands, so a lone hitch (a load spike, an
    // uninject stall) doesn't flatten the whole trace; outlier samples peg at the top instead.
    let p99_ms = {
        let mut sorted: Vec<f32> = samples.iter().map(|&(_, ms)| ms).collect();
        sorted.sort_by(f32::total_cmp);
        sorted[(sorted.len() - 1).min(sorted.len() * 99 / 100)]
    };
    // Snap the scale to the band comfortably above the p99 (not at it), so the steady-state
    // trace rides mid-chart instead of hugging the ceiling.
    let scale_ms = [8.0f32, 16.0, 24.0, 33.0, 50.0, 100.0, 200.0]
        .into_iter()
        .find(|&s| s >= p99_ms * 1.3)
        .unwrap_or(p99_ms * 1.3);

    let (response, painter) = ui.allocate_painter(
        egui::vec2(ui.available_width().max(160.0), 72.0),
        egui::Sense::hover(),
    );
    let rect = response.rect;
    let visuals = ui.visuals();
    painter.rect_filled(rect, 2.0, visuals.extreme_bg_color);

    // Reference lines at the common HMD frame budgets that fit the scale.
    for budget in [1000.0 / 120.0, 1000.0 / 90.0, 1000.0 / 72.0, 1000.0 / 45.0] {
        if budget < scale_ms {
            let y = rect.bottom() - rect.height() * (budget / scale_ms);
            painter.hline(
                rect.x_range(),
                y,
                egui::Stroke::new(1.0, visuals.faint_bg_color.gamma_multiply(2.0)),
            );
        }
    }

    // Per-segment colour by the frame-budget band the sample lands in (the same budgets as the
    // reference lines): 120 Hz and better in green through 45 Hz in orange, red beyond.
    let band_color = |ms: f32| -> egui::Color32 {
        if ms <= 1000.0 / 120.0 {
            egui::Color32::from_rgb(0x4C, 0xC9, 0x60)
        } else if ms <= 1000.0 / 90.0 {
            egui::Color32::from_rgb(0x9A, 0xCD, 0x4C)
        } else if ms <= 1000.0 / 72.0 {
            egui::Color32::from_rgb(0xD9, 0xC8, 0x3C)
        } else if ms <= 1000.0 / 45.0 {
            egui::Color32::from_rgb(0xE0, 0x8A, 0x33)
        } else {
            egui::Color32::from_rgb(0xE0, 0x4C, 0x3C)
        }
    };
    // Filled bars from the baseline (taller = slower), bucketed per pixel column so the render
    // cost is bounded by the chart width; each column takes its bucket's worst sample.
    let columns = rect.width().floor().max(1.0) as usize;
    let mut column_worst = vec![None::<f32>; columns];
    for &(t, ms) in samples.iter() {
        let age = now.duration_since(t).as_secs_f32();
        let x = ((1.0 - age / CHART_WINDOW_S) * columns as f32) as usize;
        if let Some(slot) = column_worst.get_mut(x.min(columns - 1)) {
            *slot = Some(slot.map_or(ms, |prev: f32| prev.max(ms)));
        }
    }
    for (i, ms) in column_worst.iter().enumerate() {
        let Some(ms) = *ms else { continue };
        let x = rect.left() + i as f32;
        let top = rect.bottom() - rect.height() * (ms / scale_ms).clamp(0.0, 1.0);
        painter.line_segment(
            [egui::pos2(x, rect.bottom()), egui::pos2(x, top)],
            egui::Stroke::new(1.0, band_color(ms)),
        );
    }
    painter.text(
        rect.left_top() + egui::vec2(4.0, 2.0),
        egui::Align2::LEFT_TOP,
        format!("{scale_ms:.0} ms · avg {avg_ms:.1} · p99 {p99_ms:.1} · peak {max_ms:.1}"),
        egui::FontId::proportional(10.0),
        visuals.weak_text_color(),
    );
}
