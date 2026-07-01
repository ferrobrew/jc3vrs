use std::{ffi::c_void, sync::OnceLock};

use windows::Win32::{
    Foundation::HMODULE,
    System::{LibraryLoader::DisableThreadLibraryCalls, SystemServices::DLL_PROCESS_ATTACH},
    UI::Input::KeyboardAndMouse::{VK_F5, VK_F6},
};

use crate::egui_impl::EguiState;

pub mod egui_impl;
pub mod module;
pub mod ui;
pub mod util;

mod capture;
mod config;
mod crash;
mod debug;
mod fsr;
mod hooks;
mod hud;
mod lifecycle;
mod logging;
mod stereo;

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
    crash::install();
    hooks::install();
}

/// Called to undo `initialize_startup` and eject
fn shutdown_startup() {
    // The cleanups cleared render-thread-driven config flags (e.g. the HUD redirect). Give the still-
    // live hooks a few frames to tick those changes through -- the per-frame restore runs on the
    // render thread -- before uninstalling.
    std::thread::sleep(std::time::Duration::from_millis(100));

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
    ui::render::install();
    hud::install();
    capture::install();
    tracing::info!("Initialized in game thread");

    Ok(())
}

/// Called to undo `initialize_from_game`; called once shutdown is triggered
fn shutdown_from_game() {
    if let Some(egui_state) = EguiState::get().as_mut() {
        lifecycle::run_cleanups(&mut egui_state.egui_renderer);
    }
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
            // While the F10 capture mode is active, keep input with the game (no egui capture
            // toggle) but still run the egui window so the eye-texture maintenance in
            // `prepare_if_necessary` keeps the per-eye captures sized correctly. The overlay
            // itself is hidden by skipping `egui_state.render()` in `graphics_flip` while capture
            // is active.
            if util::is_pressed(VK_F6) && !crate::capture::is_active() {
                egui_state.toggle_game_input_capture();
            }

            egui_state.run(|ctx, renderer| {
                egui::Window::new("Debug")
                    .default_pos(egui::pos2(0.0, 0.0))
                    .show(ctx, |ui| ui::egui_debug_window(ui, renderer));
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
