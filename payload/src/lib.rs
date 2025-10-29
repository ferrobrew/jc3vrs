use std::{ffi::c_void, sync::OnceLock};

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
                egui::Window::new("Hello world!").show(ctx, |ui| unsafe {
                    if let Some(gcm) = jc3gi::camera::game_camera_manager::GameCameraManager::get()
                    {
                        let next_camera_context = &gcm.m_ControlContext.m_NextCameraContext;

                        let next_transform: glam::Mat4 =
                            next_camera_context.m_CameraTransform.into();
                        let aim_transform: glam::Mat4 =
                            next_camera_context.m_AlternateAimTransform.into();
                        let listener_transform: glam::Mat4 =
                            next_camera_context.m_ListenerTransform.into();

                        let fov = next_camera_context.m_FOV.to_degrees();

                        ui.label(format!("Next transform: {next_transform:?}"));
                        ui.label(format!("Aim transform: {aim_transform:?}"));
                        ui.label(format!("Listener transform: {listener_transform:?}"));
                        ui.label(format!("FOV: {fov}"));

                        if let Some(character) =
                            jc3gi::character::character::Character::get_local_player_character()
                                .as_mut()
                        {
                            let mut head_position = jc3gi::types::math::Vector3::default();
                            character.get_head_position(&mut head_position);
                            let head_position: glam::Vec3 = head_position.into();

                            ui.label(format!("Head position: {head_position:?}"));
                        }

                        {
                            let mut cs = hooks::camera::CAMERA_SETTINGS.lock();
                            use egui::Slider;
                            ui.add(Slider::new(&mut cs.head_offset.x, -1.0..=1.0).text("Head X"));
                            ui.add(Slider::new(&mut cs.head_offset.y, -1.0..=1.0).text("Head Y"));
                            ui.add(Slider::new(&mut cs.head_offset.z, -1.0..=1.0).text("Head Z"));

                            ui.add(Slider::new(&mut cs.body_offset.x, -1.0..=1.0).text("Body X"));
                            ui.add(Slider::new(&mut cs.body_offset.y, -1.0..=1.0).text("Body Y"));
                            ui.add(Slider::new(&mut cs.body_offset.z, -1.0..=1.0).text("Body Z"));

                            ui.add(egui::Checkbox::new(&mut cs.blurs_enabled, "Blurs"));
                        }
                    }
                });
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

pub fn render() {}
