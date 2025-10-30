use std::{ffi::c_void, sync::OnceLock};

use parking_lot::Mutex;
use windows::Win32::{
    Foundation::HMODULE,
    System::{LibraryLoader::DisableThreadLibraryCalls, SystemServices::DLL_PROCESS_ATTACH},
    UI::Input::KeyboardAndMouse::{VK_F5, VK_F6},
};

use crate::egui_impl::EguiState;

pub mod egui_impl;
pub mod module;
pub mod util;

mod hooks;
mod logging;

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "system" fn DllMain(module: HMODULE, reason: u32, _unk: *mut c_void) -> bool {
    if reason == DLL_PROCESS_ATTACH {
        unsafe {
            DisableThreadLibraryCalls(module).ok();
            module::set(module);
        };
    }
    true
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "system" fn run(_: *mut c_void) {
    initialize_startup();
}

/// Called when the DLL is loaded
fn initialize_startup() {
    std::panic::set_hook(Box::new(|info| {
        let payload = info.payload();

        #[allow(clippy::manual_map)]
        let payload = if let Some(s) = payload.downcast_ref::<&str>() {
            Some(&**s)
        } else if let Some(s) = payload.downcast_ref::<String>() {
            Some(s.as_str())
        } else {
            None
        };

        let location = info.location().map(|l| l.to_string());
        let backtrace = std::backtrace::Backtrace::capture();

        tracing::error!(
            panic.payload = payload,
            panic.location = location,
            panic.backtrace = tracing::field::display(backtrace),
            "A panic occurred",
        );
    }));

    logging::install();
    tracing::info!("JC3VRS startup");
    hooks::install();
}

/// Called to undo `initialize_startup` and eject
fn shutdown_startup() {
    tracing::info!("Uninstalling hooks");
    hooks::uninstall();

    // Wait to ensure we're clear of the blast radius of the hooks
    std::thread::sleep(std::time::Duration::from_millis(100));

    tracing::info!("Ejecting");
    logging::uninstall();
    module::exit();
}

/// Called when we're on the game thread for the first time
fn initialize_from_game() -> anyhow::Result<()> {
    static INITIALIZED: OnceLock<bool> = OnceLock::new();
    if INITIALIZED.get().is_some() {
        return Ok(());
    }
    INITIALIZED.set(true).unwrap();

    EguiState::install()?;
    tracing::info!("Initialized in game thread");

    Ok(())
}

/// Called to undo `initialize_from_game`; called once shutdown is triggered
fn shutdown_from_game() {
    EguiState::uninstall();
}

/// Request that we shut down and exit
fn shutdown() {
    static SHUTDOWN: OnceLock<bool> = OnceLock::new();
    if SHUTDOWN.get().is_some() {
        return;
    }
    SHUTDOWN.set(true).unwrap();

    tracing::info!("Shutting down");
    shutdown_from_game();
    std::thread::spawn(shutdown_startup);
}

fn update() {
    if let Err(e) = initialize_from_game() {
        tracing::error!("Failed to initialize in game thread, shutting down: {e:?}");
        shutdown();
        return;
    }

    let panic = std::panic::catch_unwind(|| {
        if util::is_pressed(VK_F5) {
            shutdown();
            return;
        }

        if let Some(egui_state) = EguiState::get().as_mut() {
            if util::is_pressed(VK_F6) {
                egui_state.toggle_game_input_capture();
            }

            egui_state.run(|ctx| {
                egui::Window::new("Debug").show(ctx, |ui| egui_debug_window(ui));
            });
        }
    });
    if let Err(e) = panic {
        let panic_msg = e
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()));
        tracing::error!("Panic in update, shutting down: {panic_msg:?}");
        shutdown();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EguiTab {
    Camera,
    Game,
}
impl std::fmt::Display for EguiTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
static EGUI_TAB: Mutex<EguiTab> = Mutex::new(EguiTab::Camera);
impl EguiTab {
    fn ui(ui: &mut egui::Ui) {
        let mut tab = EGUI_TAB.lock();
        ui.horizontal(|ui| {
            for candidate_tab in [EguiTab::Camera, EguiTab::Game] {
                if ui
                    .add(
                        egui::Button::new(candidate_tab.to_string())
                            .selected(*tab == candidate_tab),
                    )
                    .clicked()
                {
                    *tab = candidate_tab;
                }
            }
        });
    }

    fn get() -> EguiTab {
        *EGUI_TAB.lock()
    }
}

pub fn egui_debug_window(ui: &mut egui::Ui) {
    EguiTab::ui(ui);

    match EguiTab::get() {
        EguiTab::Camera => {
            egui_debug_camera(ui);
        }
        EguiTab::Game => {
            egui_debug_game(ui);
        }
    }
}

fn egui_debug_camera(ui: &mut egui::Ui) {
    let mut cs = hooks::camera::CAMERA_SETTINGS.lock();
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

fn egui_debug_game(ui: &mut egui::Ui) {
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
