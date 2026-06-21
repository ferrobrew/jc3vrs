//! The "Environment" debug tab: live time-of-day and weather controls.
//!
//! Reads/writes the `WorldTime` and `Weather` singletons directly. The engine's day-cycle and
//! `WeatherController` keep running while this is open, so each value is re-read every frame and
//! some may drift back unless held (speed 0 freezes the clock; a named weather event would be
//! needed to truly pin the weather).

use jc3gi::environment::{LandscapeManager, Weather, WorldTime};

/// Render the Environment tab body.
pub(crate) fn render(ui: &mut egui::Ui) {
    fn time_of_day_ui(ui: &mut egui::Ui) {
        ui.heading("Time of day");
        let wt = unsafe { WorldTime::get() };
        let Some(wt) = wt else {
            ui.label("WorldTime singleton unavailable.");
            return;
        };
        let mut hour = wt.m_CurrentTimeOfDay;
        if ui
            .add(egui::Slider::new(&mut hour, 0.0..=24.0).text("Hour"))
            .changed()
        {
            // SetTimeOfDay fmods to 24, fires the per-hour event, and respects the pause field.
            unsafe { wt.SetTimeOfDay(hour) };
        }
        ui.add(
            egui::Slider::new(&mut wt.m_SpeedMultiplicator, 0.0..=20.0)
                .text("Day-cycle speed (0 = frozen)"),
        );
    }

    fn weather_ui(ui: &mut egui::Ui) {
        ui.heading("Weather");
        let Some(w) = weather() else {
            ui.label("Weather (LandscapeManager -> Atmosphere -> GetWeather) unavailable.");
            return;
        };
        let mut severity = w.m_Severity;
        if ui
            .add(
                egui::Slider::new(&mut severity, 0.0..=4.0)
                    .text("Severity (~0.1 clear .. ~4 storm)"),
            )
            .changed()
        {
            unsafe { w.SetSeverity(severity) };
        }
        ui.add(egui::Slider::new(&mut w.m_RainIntensity, 0.0..=1.0).text("Rain intensity"));
        ui.add(egui::Slider::new(&mut w.m_SnowRatio, 0.0..=1.0).text("Snow ratio"));
        ui.label(
            "The WeatherController pulls these back toward its own target each frame unless a \
                     named weather event holds the state.",
        );
    }

    time_of_day_ui(ui);
    ui.separator();
    weather_ui(ui);
}

/// Resolve the live `Weather`: `LandscapeManager` -> `m_Atmosphere` -> `GetWeather`.
fn weather() -> Option<&'static mut Weather> {
    unsafe {
        LandscapeManager::get()?
            .m_Atmosphere
            .as_mut()?
            .GetWeather()
            .as_mut()
    }
}
