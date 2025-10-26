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
    }

    if let Some(egui_state) = EguiState::get().as_mut() {
        egui_state.run(|ctx| {
            egui::Window::new("Hello world!").show(ctx, |ui| {
                ui.label("Hi from egui!");
                ui.spinner();
            });
        });
    }

    if util::is_pressed(VK_F5) {
        shutdown();
    } else if util::is_pressed(VK_F6)
        && let Some(egui_state) = EguiState::get().as_mut()
    {
        egui_state.toggle_game_input_capture();
    }
}

pub fn render() {}
