//! The Game tab: live game/clock state and patch toggles.

use std::sync::atomic::Ordering;

use jc3gi::{
    character::character::{AimState, Character, get_Character_EnableLocoStrafing},
    input::locomotion::{get_LocoUtil_NoAimStrafeMaxAngle, get_LocoUtil_NoAimStrafeMaxAngleAlt},
};

use crate::{
    config,
    hooks::{
        self,
        input::locomotion::{
            AIM_RELATIVE_TASK_CALLS, FACE_CAMERA_CALLS, INSTANT_SPEED_FLOORS, MOVE_TASK_CALLS,
            SHIMMED_CALLS, SKIPPED_STARTS, SLIDE_CALLS,
        },
    },
};

pub fn egui_debug_game(ui: &mut egui::Ui) {
    unsafe {
        let Some(game) = jc3gi::game::Game::get() else {
            return;
        };
        let Some(clock) = jc3gi::clock::Clock::get() else {
            return;
        };

        ui.heading("Movement");
        {
            let mut cfg = config::CONFIG.lock();
            ui.checkbox(
                &mut cfg.movement.force_fps_movement,
                "FPS movement (force aim-relative locomotion on foot)",
            )
            .on_hover_text(
                "Force the aim flags only while each locomotion task updates, so movement queues \
                 the aim-relative (strafe) acts. Known gaps: the acts are combat-stance \
                 animations (arms raised), and the no-aim strafe animations are cut content, so \
                 holstered legs stay forward-run.",
            );
            ui.checkbox(
                &mut cfg.movement.face_camera,
                "Face camera (drive the body yaw from the camera)",
            )
            .on_hover_text(
                "Write the camera's ground-plane forward to the target-face-dir blackboard value \
                 and force the game's orientation executor into its face-dir tracking mode for \
                 the local player -- the native rate-limited turn code does the rotating, in \
                 every on-foot state, holstered included.",
            );
            ui.add(
                egui::Slider::new(&mut cfg.movement.face_camera_turn_step, 0.5..=45.0)
                    .text("Face-camera turn step (deg/update)"),
            );
            ui.add(
                egui::Slider::new(&mut cfg.movement.face_camera_input_cone_deg, 0.0..=180.0)
                    .text("Face-camera input cone (deg; 180 = always pin)"),
            )
            .on_hover_text(
                "While moving, the pin only applies when the move direction is within this cone \
                 of camera-forward; outside it, the native steer runs (turn-and-run). Idle \
                 always pins. 180 pins everything; pair with slide strafe.",
            );
            ui.checkbox(
                &mut cfg.movement.slide_strafe,
                "Slide strafe (redirect movement along input while pinned)",
            )
            .on_hover_text(
                "Override the movement task's displacement direction with the input direction \
                 (the native speed envelope still applies) and keep the legs in the plain \
                 forward-run act. Lateral/backward movement slides without matching leg \
                 animations -- the game ships no neutral strafe clips.",
            );
            ui.add(
                egui::Slider::new(&mut cfg.movement.slide_rotation_deg, -180.0..=180.0)
                    .text("Slide rotation (deg)"),
            )
            .on_hover_text(
                "Yaw correction applied to the slide direction. Dial until W slides away from \
                 the camera and D slides right; the consuming frame's convention is unpinned.",
            );
            ui.checkbox(
                &mut cfg.movement.slide_instant_speed,
                "Instant speed (skip the ramp-up while sliding)",
            )
            .on_hover_text(
                "Floor the movement speed to the blackboard target while sliding. The native \
                 speed is the animation's root velocity, so the run-start clips ramp it from \
                 zero; the floor makes the motion uniform from the first frame.",
            );
            ui.checkbox(
                &mut cfg.movement.slide_skip_starts,
                "Skip start wind-up (queue the run cycle directly)",
            )
            .on_hover_text(
                "Replace the directional run-start acts with the plain forward move act while \
                 sliding, when the animation state machine accepts it (the game's own TryAct \
                 pre-flight guards the swap; the native starts run as the fallback).",
            );
        }
        // The game's own relaxed (no-aim) strafe support, left in release from the dev menu:
        // while enabled, `QueueMoveActions` queues `ACT_MOVE_NO_AIM_STRAFE` -- a neutral-stance
        // strafe act -- instead of steering the body, when the move direction is within the
        // threshold window of the body's forward. Widening both thresholds to 180 makes every
        // direction attempt the relaxed strafe: the experiment for whether neutral-stance strafe
        // animations exist beyond the stock near-backpedal window.
        patchbox(
            ui,
            "Relaxed strafe (the game's no-aim strafe acts)",
            get_Character_EnableLocoStrafing(),
        );
        patch_slider(
            ui,
            "Relaxed strafe window (deg)",
            get_LocoUtil_NoAimStrafeMaxAngle(),
            0.0..=180.0,
        );
        patch_slider(
            ui,
            "Relaxed strafe window, alt rule state (deg)",
            get_LocoUtil_NoAimStrafeMaxAngleAlt(),
            0.0..=180.0,
        );
        ui.label(format!(
            "Loco task calls: move {}  aim-relative {}  shimmed {}  face-camera {}  slide {}  \
             starts-skipped {}  speed-floors {}",
            MOVE_TASK_CALLS.load(Ordering::Relaxed),
            AIM_RELATIVE_TASK_CALLS.load(Ordering::Relaxed),
            SHIMMED_CALLS.load(Ordering::Relaxed),
            FACE_CAMERA_CALLS.load(Ordering::Relaxed),
            SLIDE_CALLS.load(Ordering::Relaxed),
            SKIPPED_STARTS.load(Ordering::Relaxed),
            INSTANT_SPEED_FLOORS.load(Ordering::Relaxed),
        ));
        match hooks::input::locomotion::debug_blackboard_snapshot() {
            Some(snapshot) => {
                let vec = |v: Option<glam::Vec3>| {
                    v.map_or_else(
                        || "<absent>".to_owned(),
                        |v| format!("({:+.2}, {:+.2}, {:+.2})", v.x, v.y, v.z),
                    )
                };
                let float = |f: Option<f32>| {
                    f.map_or_else(|| "<absent>".to_owned(), |f| format!("{f:+.2}"))
                };
                ui.label(format!(
                    "Loco (game-thread capture): input {}  move dir {}  speed {}  aux {}",
                    float(snapshot.input_magnitude),
                    vec(snapshot.move_dir),
                    float(snapshot.speed),
                    float(snapshot.aux_float),
                ));
            }
            None => {
                ui.label("Loco: <not captured yet>");
            }
        }
        if let Some(character) = Character::GetLocalPlayerCharacter().as_ref() {
            // The flag values come from the generated bitflags, so the labels can never drift from
            // the pyxis definition; only the display names are local.
            const FLAG_NAMES: [(AimState, &str); 4] = [
                (AimState::m_AimingEnabled, "Enabled"),
                (AimState::m_AimingWeapon, "Weapon"),
                (AimState::m_AimingGrapple, "Grapple"),
                (AimState::m_WasAiming, "WasAiming"),
            ];
            let flags = character.m_AimFlags;
            let active: Vec<&str> = FLAG_NAMES
                .iter()
                .filter(|(flag, _)| flags.contains(*flag))
                .map(|(_, name)| *name)
                .collect();
            ui.label(format!(
                "Game aim flags {:#04x}: {}  timer: {:.2}s (real state; the shim's force is \
                 scoped to the loco tasks and never visible here)",
                flags.bits(),
                active.join(" | "),
                character.m_AimTimer,
            ));
        }

        ui.heading("Game");
        ui.label(format!("Update frequency: {}Hz", game.m_UpdateFrequency));
        ui.label(format!("Update flags: {:X}", game.m_UpdateFlags));
        ui.label(format!(
            "Interpolation method: {:X}",
            game.m_InterpolationMethod
        ));
        {
            let mut interpolation_override = game.m_InterpolationOverride;
            let before = interpolation_override;
            egui::ComboBox::from_label("Interpolation override")
                .selected_text(interpolation_override.to_string())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut interpolation_override, -1, "Really None");
                    ui.selectable_value(&mut interpolation_override, 0, "None");
                    ui.selectable_value(&mut interpolation_override, 1, "1");
                    ui.selectable_value(&mut interpolation_override, 2, "2");
                    ui.selectable_value(&mut interpolation_override, 3, "3");
                });

            if before != interpolation_override
                && let Some(mut patcher) = hooks::patcher()
            {
                patcher.patch(
                    &mut game.m_InterpolationOverride as *mut _ as usize,
                    &interpolation_override.to_le_bytes(),
                );
            }
        }
        patchbox(ui, "Decouple enabled", &mut game.m_DecoupleEnabled);

        ui.heading("Clock");
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
}

fn patchbox(ui: &mut egui::Ui, label: &str, value: *mut bool) {
    let mut enabled = unsafe { *value };
    if ui.checkbox(&mut enabled, label).changed()
        && let Some(mut patcher) = hooks::patcher()
    {
        unsafe {
            patcher.patch(value as *const _ as usize, &[if enabled { 1 } else { 0 }]);
        }
    }
}

/// A slider over a game float global, written through the patcher like [`patchbox`] so pages that
/// are not plainly writable still take the edit.
fn patch_slider(
    ui: &mut egui::Ui,
    label: &str,
    value: *mut f32,
    range: std::ops::RangeInclusive<f32>,
) {
    let mut v = unsafe { *value };
    if ui
        .add(egui::Slider::new(&mut v, range).text(label))
        .changed()
        && let Some(mut patcher) = hooks::patcher()
    {
        unsafe {
            patcher.patch(value as *const _ as usize, &v.to_le_bytes());
        }
    }
}
