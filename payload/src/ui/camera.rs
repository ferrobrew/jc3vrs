//! The Camera tab: VR head/body camera settings, plus the shared matrix-grid widget.

use crate::config;

pub fn egui_debug_camera(ui: &mut egui::Ui) {
    let mut cfg = config::CONFIG.lock();
    let cs = &mut cfg.camera;
    ui.checkbox(&mut cs.enabled, "Enabled");
    ui.checkbox(&mut cs.always_use_t1, "Always use T1");
    ui.checkbox(&mut cs.blurs_enabled, "Blurs");
    ui.checkbox(&mut cs.use_eye_matrices, "Use eye matrices");

    ui.add_enabled_ui(!cs.use_eye_matrices, |ui| {
        use egui::Slider;
        ui.add(Slider::new(&mut cs.head_offset.x, -1.0..=1.0).text("Head X"));
        ui.add(Slider::new(&mut cs.head_offset.y, -1.0..=1.0).text("Head Y"));
        ui.add(Slider::new(&mut cs.head_offset.z, -1.0..=1.0).text("Head Z"));

        ui.add(Slider::new(&mut cs.body_offset.x, -1.0..=1.0).text("Body X"));
        ui.add(Slider::new(&mut cs.body_offset.y, -1.0..=1.0).text("Body Y"));
        ui.add(Slider::new(&mut cs.body_offset.z, -1.0..=1.0).text("Body Z"));
    });
}

pub fn matrix_grid(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    m: &[f32; 16],
    other: Option<&[f32; 16]>,
) {
    ui.label(label);
    egui::Grid::new(id).striped(true).show(ui, |ui| {
        for r in 0..4 {
            for c in 0..4 {
                let i = r * 4 + c;
                let v = m[i];
                let differs = other.is_some_and(|o| (v - o[i]).abs() > 1e-5);
                let text = format!("{v:+.3}");
                if differs {
                    ui.colored_label(egui::Color32::YELLOW, text);
                } else {
                    ui.label(text);
                }
            }
            ui.end_row();
        }
    });
}
