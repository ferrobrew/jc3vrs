//! The Performance tab: the engine clock's frame-rate readouts and pacing controls, kept small so
//! it can be dragged out into its own window and watched while tuning render features.

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
