//! The Camera tab: VR head/body camera settings, plus the shared matrix-grid widget.

use egui::Slider;

use crate::{config, headpose, hooks};

pub fn egui_debug_camera(ui: &mut egui::Ui) {
    let mut cfg = config::CONFIG.lock();
    let cs = &mut cfg.camera;
    ui.checkbox(&mut cs.enabled, "Enabled");
    ui.checkbox(&mut cs.always_use_t1, "Always use T1");
    ui.checkbox(&mut cs.blurs_enabled, "Blurs");
    ui.checkbox(&mut cs.use_eye_matrices, "Use eye matrices");

    ui.add_enabled_ui(!cs.use_eye_matrices, |ui| {
        ui.add(Slider::new(&mut cs.head_offset.x, -1.0..=1.0).text("Head X"));
        ui.add(Slider::new(&mut cs.head_offset.y, -1.0..=1.0).text("Head Y"));
        ui.add(Slider::new(&mut cs.head_offset.z, -1.0..=1.0).text("Head Z"));

        ui.add(Slider::new(&mut cs.body_offset.x, -1.0..=1.0).text("Body X"));
        ui.add(Slider::new(&mut cs.body_offset.y, -1.0..=1.0).text("Body Y"));
        ui.add(Slider::new(&mut cs.body_offset.z, -1.0..=1.0).text("Body Z"));
    });

    ui.separator();
    egui_debug_headpose(ui, &mut cfg.headpose);
}

fn egui_debug_headpose(ui: &mut egui::Ui, hp: &mut headpose::HeadPoseConfig) {
    ui.heading("Headpose");

    ui.checkbox(&mut hp.enabled, "Enabled");
    ui.label(format!("Mode: {:?}", headpose::sim::mode()));
    ui.label(format!("Latch: {:?}", headpose::sim::latch_state()));

    let (yaw, pitch, roll) = headpose::sim::euler_angles();
    ui.label(format!("Yaw (body-relative): {:+.1}°", yaw.to_degrees()));
    ui.label(format!("Pitch: {:+.1}°", pitch.to_degrees()));
    ui.label(format!("Roll:  {:+.1}°", roll.to_degrees()));

    let pose = headpose::query();
    ui.label(format!(
        "Position: ({:+.2}, {:+.2}, {:+.2})",
        pose.position.x, pose.position.y, pose.position.z
    ));

    ui.label(match headpose::anchor() {
        Some(anchor) => format!(
            "Anchor: ({:+.2}, {:+.2}, {:+.2})",
            anchor.x, anchor.y, anchor.z
        ),
        None => "Anchor: none".to_string(),
    });
    // The engine's sub-frame interpolation fraction (issue #20): stuck at 0 or 1 means the
    // engine's camera lerp is inert and the sim-tick cadence shows as judder.
    ui.label(format!("Camera dtf: {:.3}", hooks::camera::last_dtf()));

    if ui.button("Recenter").clicked() {
        headpose::recenter();
    }

    ui.add(Slider::new(&mut hp.latch_threshold_deg, 0.0..=180.0).text("Latch threshold (°)"));
    ui.add(
        Slider::new(&mut hp.latch_disengage_threshold_deg, 0.0..=180.0)
            .text("Latch disengage threshold (°)"),
    );
    ui.add(
        Slider::new(&mut hp.free_look_yaw_limit_deg, 0.0..=180.0).text("Free-look yaw limit (°)"),
    );
    ui.add(
        Slider::new(&mut hp.free_look_pitch_limit_deg, 0.0..=180.0)
            .text("Free-look pitch limit (°)"),
    );
    ui.add(
        Slider::new(&mut hp.mouse_sensitivity, 1.0..=20.0)
            .step_by(1.0)
            .text("Mouse sensitivity (°/unit)"),
    );
    ui.checkbox(&mut hp.invert_y, "Invert Y");
    ui.add(Slider::new(&mut hp.position_offset.x, -1.0..=1.0).text("Position offset X"));
    ui.add(Slider::new(&mut hp.position_offset.y, -1.0..=1.0).text("Position offset Y"));
    ui.add(Slider::new(&mut hp.position_offset.z, -1.0..=1.0).text("Position offset Z"));
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
