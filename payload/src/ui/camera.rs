//! The Camera tab: VR head/body camera settings, plus the shared matrix-grid widget.

use egui::Slider;
use jc3gi::types::math::Matrix4;

use crate::{
    config, grapple, headpose, headpose::config::VrTurnMode, hooks, hooks::character::BodyIkConfig,
    logging, vr, vr::FreezeMode,
};

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

    ui.collapsing("Headpose", |ui| egui_debug_headpose(ui, &mut cfg.headpose));
    ui.collapsing("Frozen pose (diagnostic)", |ui| {
        egui_frozen_pose(ui, &mut cfg.vr);
    });
    ui.collapsing("Grapple reel-in comfort", |ui| {
        egui_grapple(ui, &mut cfg.headpose.grapple);
    });
    ui.collapsing("VR body turn", |ui| {
        egui_vr_turn(ui, &mut cfg.headpose.vr_turn);
    });
    ui.collapsing("Body IK", |ui| egui_debug_body_ik(ui, &mut cfg.body_ik));
}

fn egui_debug_body_ik(ui: &mut egui::Ui, ik: &mut BodyIkConfig) {
    ui.checkbox(&mut ik.enabled, "Enabled")
        .on_hover_text("Drive the upper body toward the headpose via the engine's HumanIK solver.");
    ui.checkbox(
        &mut ik.rotation_target,
        "Rotation target (aim head at headpose)",
    );
    ui.add(Slider::new(&mut ik.weight, 0.0..=1.0).text("Master weight"));
    ui.add(Slider::new(&mut ik.head_reach_t, 0.0..=1.0).text("Head reach (translation)"));
    ui.add(Slider::new(&mut ik.head_reach_r, 0.0..=1.0).text("Head reach (rotation)"));
    ui.add(Slider::new(&mut ik.head_pull, 0.0..=1.0).text("Head pull (body follow)"))
        .on_hover_text(
            "How much reaching the head target carries into the shoulders, chest, and spine. At \
             zero the neck absorbs it all and the torso does not move.",
        );
    ui.add(Slider::new(&mut ik.head_resist, 0.0..=1.0).text("Head resist"))
        .on_hover_text("How much the head resists being displaced by other effectors' pull.");
    ui.checkbox(&mut ik.interpolation, "Interpolation (ease reach in)");
    ui.add(Slider::new(&mut ik.interpolation_rate, 0.0..=10.0).text("Interpolation rate"));
    ui.checkbox(&mut ik.blend_out, "Blend out").on_hover_text(
        "Ease the reach back out when a target stops being supplied. Off by default: the \
             engine keeps a blending-out target alive with the stale pose it was queued with, so \
             every gate flip leaves the hands chasing an old animation frame for the decay tail.",
    );
    ui.add(Slider::new(&mut ik.blend_out_rate, 0.0..=10.0).text("Blend-out rate"));
    ui.add(Slider::new(&mut ik.lean_share, 0.0..=1.0).text("Torso lean share"))
        .on_hover_text(
            "How much of your lean the spine takes, as a swing about its base. This is what keeps \
             the torso under the camera; the rest is absorbed by the neck.",
        );
    ui.add(Slider::new(&mut ik.pivot_drop, 0.0..=1.0).text("Pivot drop (spine → hips)"))
        .on_hover_text(
            "Where the torso bends. 0 is the base of the spine, right for a sideways lean; higher \
             moves it toward the hips, which is what a forward lean actually hinges about. Lower \
             pivot means less angle but more shoulder travel.",
        );
    ui.add(Slider::new(&mut ik.lean_max_deg, 0.0..=90.0).text("Torso lean max (deg)"))
        .on_hover_text(
            "The spine's range of motion. Also bounds the lean model where it stops being valid: \
             with the head near or below the pivot's height there is no sane spine bend, so it \
             saturates here rather than folding over.",
        );
    ui.add(Slider::new(&mut ik.yaw_share, 0.0..=1.0).text("Torso yaw share"))
        .on_hover_text("How much of the head's turn the torso takes, about the same pivot.");
    ui.add(Slider::new(&mut ik.shoulder_reach_t, 0.0..=1.0).text("Shoulder reach (translation)"));
    ui.add(Slider::new(&mut ik.shoulder_pull, 0.0..=1.0).text("Shoulder pull (into the spine)"));
    ui.add(Slider::new(&mut ik.chest_reach_r, 0.0..=1.0).text("Chest reach (rotation)"));
    ui.checkbox(&mut ik.defer_while_aiming, "Defer to the game while aiming")
        .on_hover_text(
            "Stand the torso targets and hand pins down while aiming a weapon or the grapple, \
             leaving only the head target. The game's aim IK needs the arm chain free to swing the \
             weapon onto the aim direction; holding the wrists and girdle clamps aiming to the \
             authored sweep cone.",
        );
    ui.checkbox(&mut ik.pin_hands, "Pin hands to the animated pose")
        .on_hover_text(
            "Hold the hands where the animation put them while the shoulders move, so the arms \
             articulate instead of the hands sliding off the wheel.",
        );
    ui.add(Slider::new(&mut ik.hand_reach_t, 0.0..=1.0).text("Hand pin (position)"));
    ui.add(Slider::new(&mut ik.hand_reach_r, 0.0..=1.0).text("Hand pin (rotation)"))
        .on_hover_text(
            "Holds the hands' animated orientation as well as their place. Without it they keep \
             station but spin, and the wielded weapon's aim spins with them.",
        );
    ui.add(Slider::new(&mut ik.hand_resist, 0.0..=1.0).text("Hand resist"))
        .on_hover_text("How much the hands refuse to be dragged by the shoulders' pull.");
    ui.add(Slider::new(&mut ik.target_offset.x, -1.0..=1.0).text("Target offset X (m)"));
    ui.add(Slider::new(&mut ik.target_offset.y, -1.0..=1.0).text("Target offset Y (m)"));
    ui.add(Slider::new(&mut ik.target_offset.z, -1.0..=1.0).text("Target offset Z (m)"));

    ui.separator();
    let target = hooks::character::BODY_IK_TARGET;
    let mut verbose = logging::is_target_debug(target);
    if ui
        .checkbox(&mut verbose, "Log body-IK state to the session log (1 Hz)")
        .on_hover_text(
            "Turns this subsystem up to DEBUG, for reading the numbers below after the fact \
             instead of mid-lean. Equivalent to a body_ik=debug directive in the Diagnostics tab's \
             filter box, without the typing.",
        )
        .changed()
        && let Err(error) = logging::set_target_debug(target, verbose)
    {
        tracing::warn!(%error, "could not change the body-IK log level");
    }

    match hooks::character::body_ik_status() {
        None => {
            ui.label("Status: the character hook has not run yet.");
        }
        Some(status) => match status.skip {
            Some(skip) => {
                ui.label(format!(
                    "Status: queuing nothing — {} (mode {:?}).",
                    skip.label(),
                    status.mode
                ));
            }
            None => {
                ui.label(format!(
                    "Mode: {:?}   State id: {:#010x}",
                    status.mode, status.state_id
                ));
                ui.label(format!(
                    "Effectors: head {}, chest end {}",
                    status.head_effector, status.chest_effector
                ));
                ui.label(format!(
                    "Head target delta: {:.3} m   Room-scale offset: {:.3} m ({:+.2}, {:+.2}, \
                     {:+.2})",
                    status.target_delta,
                    status.head_offset.length(),
                    status.head_offset.x,
                    status.head_offset.y,
                    status.head_offset.z
                ));
                ui.label(format!(
                    "Torso lean: {:.1}°   Torso yaw: {:+.1}°   Shoulders: {}/2 driven, {}/2 \
                     mapped   Chest driven: {}   Hands pinned: {}/2{}",
                    status.torso_lean_deg,
                    status.torso_yaw_deg,
                    status.shoulders_driven,
                    status.shoulders_mapped,
                    if status.chest_driven { "yes" } else { "no" },
                    status.hands_pinned,
                    if status.deferred_to_aim {
                        "   (deferring to the game's aim)"
                    } else {
                        ""
                    }
                ));
                ui.label(format!(
                    "Engine-side head values: reachT {:.2}, reachR {:.2}, pull {:.2}",
                    status.engine_reach_t, status.engine_reach_r, status.engine_pull
                ));
            }
        },
    }
}

fn egui_debug_headpose(ui: &mut egui::Ui, hp: &mut headpose::HeadPoseConfig) {
    ui.checkbox(&mut hp.enabled, "Enabled");
    ui.label(format!("Mode: {:?}", headpose::sim::mode()));
    ui.label(format!("Latch: {:?}", headpose::sim::latch_state()));

    // The body-relative rotation rather than the sim's raw angles: under the VR source the sim never
    // accumulates them, so the raw angles read as a permanent zero.
    let (yaw, pitch, roll) = headpose::body_relative_rotation().to_euler(glam::EulerRot::YXZ);
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
}

/// The frozen-pose diagnostic ([`crate::vr::pose_control`]): pin the rendered pose, then drive it by
/// hand. Content that only mis-renders *in motion* (a mis-scaled screen-space pass, a render block
/// that slides under the camera) cannot be measured by turning your head -- the same movement is never
/// repeated twice. Frozen, a pose is an exact set of numbers: dial one in, step yaw by exactly one
/// degree, and the two frames differ in exactly that.
///
/// The mode picks *what* is held: the head's contribution to the camera, or the whole camera. For a
/// "slides as the camera moves" measurement it is the full camera you want, since only that holds
/// everything the view is built from.
fn egui_frozen_pose(ui: &mut egui::Ui, vr: &mut vr::VrConfig) {
    ui.horizontal(|ui| {
        ui.label("Freeze:");
        ui.radio_value(&mut vr.freeze_mode, FreezeMode::Off, "Off");
        ui.radio_value(
            &mut vr.freeze_mode,
            FreezeMode::CockpitPose,
            "Head pose (cockpit)",
        )
        .on_hover_text(
            "Capture the current HMD pose (and the body frame and head anchor with it) and reuse it \
             every frame. Isolates HMD pose-noise-driven flicker (present even on a desk) from \
             intrinsic render artifacts. This holds the head's contribution still, not the camera: \
             a camera the game moves still moves.",
        );
        ui.radio_value(
            &mut vr.freeze_mode,
            FreezeMode::FullCamera,
            "Full camera (world)",
        )
        .on_hover_text(
            "Pin the final scene render camera in world space, at the last point before the engine \
             consumes it. Nothing moves the view -- not the head, the body, the animated head bone, \
             or the game's own camera -- and the per-eye offsets are held with it. This is the mode \
             for measuring content that only mis-renders in motion.",
        );
    });
    // While frozen, edit the pose actually driving the render. Unfrozen there is nothing to edit, so
    // the live cockpit pose stands in as a readout -- but only in the modes whose numbers are in that
    // frame. The full-camera mode has no live stand-in (the cockpit pose is a different quantity in a
    // different frame, and showing it here would read as a world position), so it says so instead.
    let frozen = vr::pose_control::current();
    let values = match (frozen, vr.freeze_mode) {
        (Some(values), _) => Some(values),
        (None, FreezeMode::FullCamera) => None,
        (None, _) => headpose::xr::cockpit_pose()
            .map(|p| vr::pose_control::PoseValues::from_pose(p.position, p.orientation)),
    };
    // The two frames are not interchangeable: cockpit metres are head travel from the recenter
    // baseline, world metres are the camera's position in the game world. A number whose frame is
    // ambiguous is worse than no number, so the frame is always stated next to the values.
    ui.label(match (frozen.is_some(), vr.freeze_mode) {
        (true, FreezeMode::FullCamera) => {
            "Frame: world -- the render camera's world-space position and orientation"
        }
        (true, _) => "Frame: cockpit -- the head pose relative to the recenter baseline",
        (false, _) => "Frame: cockpit (live head pose) -- freeze to edit",
    });
    let Some(mut values) = values else {
        ui.label(match vr.freeze_mode {
            FreezeMode::FullCamera => "Pose: no frame has rendered under the freeze yet.",
            _ => "Pose: no VR frame has rendered yet.",
        });
        return;
    };

    let step_m = vr.freeze_pose_step_m;
    let step_deg = vr.freeze_pose_step_deg;
    let mut changed = false;
    ui.add_enabled_ui(frozen.is_some(), |ui| {
        egui::Grid::new("frozen_pose_grid")
            .num_columns(4)
            .show(ui, |ui| {
                changed |= pose_row(ui, "X", &mut values.position.x, step_m, " m");
                changed |= pose_row(ui, "Y", &mut values.position.y, step_m, " m");
                changed |= pose_row(ui, "Z", &mut values.position.z, step_m, " m");
                changed |= pose_row(ui, "Yaw", &mut values.yaw_deg, step_deg, "°");
                changed |= pose_row(ui, "Pitch", &mut values.pitch_deg, step_deg, "°");
                changed |= pose_row(ui, "Roll", &mut values.roll_deg, step_deg, "°");
            });
        if changed {
            vr::pose_control::set_current(values);
        }

        if ui
            .button("Reset to captured pose")
            .on_hover_text(match vr::pose_control::base() {
                Some(base) => format!(
                    "Return to the pose captured when the freeze engaged: ({:+.3}, {:+.3}, {:+.3}) m, \
                     yaw {:+.2}°, pitch {:+.2}°, roll {:+.2}°.",
                    base.position.x,
                    base.position.y,
                    base.position.z,
                    base.yaw_deg,
                    base.pitch_deg,
                    base.roll_deg,
                ),
                None => "Nothing is frozen.".to_string(),
            })
            .clicked()
        {
            vr::pose_control::reset();
        }
    });

    ui.add(
        egui::DragValue::new(&mut vr.freeze_pose_step_m)
            .speed(0.01)
            .range(0.001..=10.0)
            .prefix("Translation step: ")
            .suffix(" m"),
    );
    ui.add(
        egui::DragValue::new(&mut vr.freeze_pose_step_deg)
            .speed(0.1)
            .range(0.01..=90.0)
            .prefix("Rotation step: ")
            .suffix("°"),
    );
}

/// One editable pose component: direct numeric entry plus exact `-`/`+` nudges of `step`. Returns
/// whether the value changed this frame.
fn pose_row(ui: &mut egui::Ui, label: &str, value: &mut f32, step: f32, suffix: &str) -> bool {
    ui.label(label);
    let mut changed = ui
        .add(
            egui::DragValue::new(value)
                .speed(step * 0.1)
                .max_decimals(4)
                .suffix(suffix),
        )
        .changed();
    if ui
        .button("-")
        .on_hover_text("Step down by exactly one step, snapped to the step grid.")
        .clicked()
    {
        *value = vr::pose_control::nudge(*value, step, -1);
        changed = true;
    }
    if ui
        .button("+")
        .on_hover_text("Step up by exactly one step, snapped to the step grid.")
        .clicked()
    {
        *value = vr::pose_control::nudge(*value, step, 1);
        changed = true;
    }
    ui.end_row();
    changed
}

/// The grapple reel-in body-frame filter (issue #36): which rotation the reel is allowed to
/// compose into the view, and the blend constants around the reel window.
fn egui_grapple(ui: &mut egui::Ui, grapple: &mut grapple::GrappleComfortConfig) {
    use crate::grapple::GrappleComfortMode;

    let mode_label = |mode: GrappleComfortMode| match mode {
        GrappleComfortMode::Off => "Off",
        GrappleComfortMode::HoldView => "Hold view",
        GrappleComfortMode::LevelPitch => "Level pitch",
    };
    egui::ComboBox::from_label("Mode")
        .selected_text(mode_label(grapple.mode))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut grapple.mode, GrappleComfortMode::Off, "Off");
            ui.selectable_value(&mut grapple.mode, GrappleComfortMode::HoldView, "Hold view")
                .on_hover_text(
                    "Cancel the body rotation the reel adds: the view stays where you were \
                     looking at reel start, and only the HMD (or mouse) moves it.",
                );
            ui.selectable_value(
                &mut grapple.mode,
                GrappleComfortMode::LevelPitch,
                "Level pitch",
            )
            .on_hover_text(
                "Keep the view level while reeling: the body's pitch and roll are dropped, but \
                 its yaw toward the target still composes.",
            );
        });
    ui.add(Slider::new(&mut grapple.engage_s, 0.0..=1.0).text("Engage (s)"));
    ui.add(Slider::new(&mut grapple.release_s, 0.0..=2.0).text("Release (s)"));
    ui.checkbox(&mut grapple.yaw_handoff, "Yaw handoff (body turns to view)")
        .on_hover_text(
            "At reel end, turn the character toward where you are looking instead of sweeping \
             the view toward the character's landing heading. VR, on foot, hold-view only.",
        );
    ui.add(Slider::new(&mut grapple.handoff_timeout_s, 0.0..=3.0).text("Handoff timeout (s)"));
    ui.add(
        Slider::new(&mut grapple.anchor_snap_threshold_mps, 0.0..=120.0)
            .text("Landing snap threshold (m/s)"),
    )
    .on_hover_text(
        "Single-step velocity change of the body-driven head position beyond which it is \
         treated as a landing snap and absorbed; steady motion of any speed passes through. \
         0 disables.",
    );
    ui.add(Slider::new(&mut grapple.anchor_snap_ease_s, 0.0..=1.0).text("Landing snap ease (s)"));
    ui.label(format!("Blend: {:.2}", grapple::blend_factor()));
    let mut log = grapple::telemetry::log_enabled();
    if ui
        .checkbox(&mut log, "Log reel telemetry")
        .on_hover_text(
            "Write per-tick filter state and per-frame pose composition to the log \
             (`grapple_telemetry` target) for offline analysis.",
        )
        .changed()
    {
        grapple::telemetry::set_log_enabled(log);
    }
}

/// The on-foot body-turn knobs used while the HMD owns the head (mouse and right stick turn the body,
/// not the head). Separate from the flatscreen latch above, which never runs under an OpenXR session.
fn egui_vr_turn(ui: &mut egui::Ui, turn: &mut headpose::config::VrTurnConfig) {
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

pub fn matrix_grid(ui: &mut egui::Ui, id: &str, label: &str, m: &Matrix4, other: Option<&Matrix4>) {
    ui.label(label);
    egui::Grid::new(id).striped(true).show(ui, |ui| {
        for r in 0..4 {
            for c in 0..4 {
                let i = r * 4 + c;
                let v = m.data[i];
                let differs = other.is_some_and(|o| (v - o.data[i]).abs() > 1e-5);
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
