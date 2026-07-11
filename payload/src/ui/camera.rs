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
    ui.checkbox(
        &mut cs.hide_head_draws,
        "Hide head (collapse facial bones; shadow keeps it)",
    );
    ui.checkbox(&mut cs.hide_head_scale, "Hide head (legacy bone scale)");
    if ui.button("Dump character block draws (log)").clicked() {
        hooks::graphics_engine::render_block::DUMP_DRAWS
            .store(48, std::sync::atomic::Ordering::Relaxed);
    }

    // With eye matrices on, the head sliders are a correction relative to the measured eye
    // position; with them off, they are the whole arm from the neck pivot.
    let head_label = if cs.use_eye_matrices {
        "Head (from eyes)"
    } else {
        "Head (from neck)"
    };
    ui.add(Slider::new(&mut cs.head_offset.x, -1.0..=1.0).text(format!("{head_label} X")));
    ui.add(Slider::new(&mut cs.head_offset.y, -1.0..=1.0).text(format!("{head_label} Y")));
    ui.add(Slider::new(&mut cs.head_offset.z, -1.0..=1.0).text(format!("{head_label} Z")));

    ui.add(Slider::new(&mut cs.body_offset.x, -1.0..=1.0).text("Body X"));
    ui.add(Slider::new(&mut cs.body_offset.y, -1.0..=1.0).text("Body Y"));
    ui.add(Slider::new(&mut cs.body_offset.z, -1.0..=1.0).text("Body Z"));

    ui.separator();
    egui_debug_headpose(ui, &mut cfg.headpose);

    ui.separator();
    egui::CollapsingHeader::new("Body IK")
        .default_open(false)
        .show(ui, |ui| egui_debug_body_ik(ui, &mut cfg.body_ik));
}

fn egui_debug_body_ik(ui: &mut egui::Ui, ik: &mut config::BodyIkConfig) {
    ui.checkbox(&mut ik.enabled, "Enabled")
        .on_hover_text("Drive the upper body toward the headpose via the engine's HumanIK solver.");
    ui.checkbox(
        &mut ik.rotation_target,
        "Rotation target (aim head at headpose)",
    );
    ui.add(Slider::new(&mut ik.weight, 0.0..=1.0).text("Master weight"));
    ui.add(Slider::new(&mut ik.head_reach_t, 0.0..=1.0).text("Head reach (translation)"));
    ui.add(Slider::new(&mut ik.head_reach_r, 0.0..=1.0).text("Head reach (rotation)"));
    ui.checkbox(&mut ik.interpolation, "Interpolation (ease reach in)");
    ui.add(Slider::new(&mut ik.interpolation_rate, 0.0..=10.0).text("Interpolation rate"));
    ui.checkbox(&mut ik.blend_out, "Blend out");
    ui.add(Slider::new(&mut ik.blend_out_rate, 0.0..=10.0).text("Blend-out rate"));
    ui.add(Slider::new(&mut ik.target_offset.x, -1.0..=1.0).text("Target offset X (m)"));
    ui.add(Slider::new(&mut ik.target_offset.y, -1.0..=1.0).text("Target offset Y (m)"));
    ui.add(Slider::new(&mut ik.target_offset.z, -1.0..=1.0).text("Target offset Z (m)"));
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
    let neck_delta = headpose::neck_delta();
    ui.label(format!(
        "Head → neck: ({:+.2}, {:+.2}, {:+.2})",
        neck_delta.x, neck_delta.y, neck_delta.z
    ));
    let eye_arm = headpose::eye_arm();
    ui.label(format!(
        "Neck → eyes (arm): ({:+.2}, {:+.2}, {:+.2})",
        eye_arm.x, eye_arm.y, eye_arm.z
    ));
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
    ui.add(Slider::new(&mut hp.neck_twist_start_deg, 0.0..=120.0).text("Neck twist start (°)"));
    ui.add(Slider::new(&mut hp.neck_twist_max_deg, 0.0..=90.0).text("Neck twist max (°)"));
    ui.checkbox(&mut hp.posture_enabled, "Body posture (invert with hangs)")
        .on_hover_text(
            "Fold the animated neck axis's swing away from body-up into the view, so hanging \
             upside down inverts the camera. Deadband keeps idle sway out.",
        );
    ui.add(Slider::new(&mut hp.posture_deadband_deg, 0.0..=90.0).text("Posture deadband (°)"));
    ui.add(Slider::new(&mut hp.posture_full_deg, 0.0..=180.0).text("Posture full at (°)"));
    ui.add(Slider::new(&mut hp.posture_smoothing_s, 0.0..=2.0).text("Posture smoothing (s)"));
    ui.add(Slider::new(&mut hp.position_offset.x, -1.0..=1.0).text("Roomscale offset X (m)"));
    ui.add(Slider::new(&mut hp.position_offset.y, -1.0..=1.0).text("Roomscale offset Y (m)"));
    ui.add(Slider::new(&mut hp.position_offset.z, -1.0..=1.0).text("Roomscale offset Z (m)"));

    egui_vr_turn(ui, &mut hp.vr_turn);
}

/// The on-foot body-turn knobs used while the HMD owns the head (mouse and right stick turn the body,
/// not the head). Separate from the flatscreen latch above, which never runs under an OpenXR session.
fn egui_vr_turn(ui: &mut egui::Ui, turn: &mut headpose::config::VrTurnConfig) {
    use headpose::config::VrTurnMode;

    ui.separator();
    ui.heading("VR body turn");
    ui.horizontal(|ui| {
        ui.label("Mode:");
        ui.radio_value(&mut turn.mode, VrTurnMode::Smooth, "Smooth");
        ui.radio_value(&mut turn.mode, VrTurnMode::Snap, "Snap");
    });
    match turn.mode {
        VrTurnMode::Smooth => {
            ui.add(
                Slider::new(&mut turn.mouse_turn_scale, 0.5..=20.0)
                    .text("Mouse turn scale (°/unit)"),
            )
            .on_hover_text(
                "Body yaw per unit of mouse-look delta. The whole-body turn is still rate-limited \
                 by the Game tab's face-camera turn step.",
            );
            ui.add(Slider::new(&mut turn.smooth_scale, 0.0..=4.0).text("Stick turn scale (×)"))
                .on_hover_text("Right-stick turn rate as a multiple of the mouse sensitivity.");
            ui.add(Slider::new(&mut turn.deadzone, 0.0..=0.5).text("Stick deadzone"));
        }
        VrTurnMode::Snap => {
            ui.add(Slider::new(&mut turn.snap_angle_deg, 5.0..=90.0).text("Snap angle (°)"));
            ui.add(Slider::new(&mut turn.snap_threshold, 0.1..=1.0).text("Snap threshold"));
        }
    }
    ui.add(Slider::new(&mut turn.max_body_lead_deg, 0.0..=180.0).text("Max body lead (°)"))
        .on_hover_text(
            "How far the turn target may lead the body's facing. Caps how long the body keeps \
             turning after a fast flick stops.",
        );
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
