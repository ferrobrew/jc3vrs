//! The Game tab: live game/clock state and patch toggles.

use crate::hooks;

pub fn egui_debug_game(ui: &mut egui::Ui) {
    unsafe {
        let Some(game) = jc3gi::game::Game::get() else {
            return;
        };
        let Some(clock) = jc3gi::clock::Clock::get() else {
            return;
        };

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
