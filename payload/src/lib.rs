use std::{
    ffi::c_void,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

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
pub mod vr;

mod allocator;
mod capture;
mod config;
mod crash;
mod debug;
mod far_field;
mod fsr;
mod grapple;
mod headpose;
mod hooks;
mod hud;
mod lifecycle;
mod logging;
#[cfg(feature = "profiler")]
mod profiler;
mod screenshot;
mod session;
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
    // Resolve this run's output directory first, so the log, crash dumps, and every other artifact
    // land under the same timestamped session folder.
    session::init();

    std::panic::set_hook(Box::new(|info| {
        // Log the location before touching the payload: for formatted panics, `payload()` runs the
        // format arguments' Display impls, and one reading dead game memory faults — which
        // previously killed the process before anything was logged. The location is plain static
        // data and always safe.
        let location = info.location().map(|l| l.to_string());
        tracing::error!(
            panic.location = location.as_deref(),
            "A panic occurred; formatting the payload (a crash before the next record means the \
             panic message itself faulted)",
        );

        let payload = info.payload();

        #[allow(clippy::manual_map)]
        let payload = if let Some(s) = payload.downcast_ref::<&str>() {
            Some(&**s)
        } else if let Some(s) = payload.downcast_ref::<String>() {
            Some(s.as_str())
        } else {
            None
        };

        let backtrace = std::backtrace::Backtrace::capture();

        tracing::error!(
            panic.payload = payload,
            panic.location = location,
            panic.backtrace = tracing::field::display(backtrace),
            "A panic occurred",
        );
    }));

    logging::install();
    tracing::info!(build = BUILD_STAMP, "JC3VRS startup");
    crash::install();
    hooks::install();
}

/// Called to undo `initialize_startup` and eject
fn shutdown_startup() {
    // Stop the frame-tail worker before anything is torn down: a thread still alive at
    // `module::exit` would be running in an unmapped image. Any in-flight tail finishes first
    // (the VR teardown in `shutdown_from_game` already synchronized on the runtime lock).
    //
    // If it will not stop, pin the module so the image is never unmapped. A live thread in an
    // unmapped image parks forever holding whatever it took, and the process wedges with nothing
    // runnable and nothing logged -- strictly worse than leaking this DLL for the remaining lifetime
    // of the game, which the player ends by quitting anyway.
    if !vr::tail::shutdown() {
        module::pin();
    }

    // Likewise for a profiler capture's writer thread: see `profiler::capture::shutdown`'s doc
    // comment for why an unjoined writer is the same class of hazard as the frame tail.
    #[cfg(feature = "profiler")]
    if !profiler::capture::shutdown() {
        module::pin();
    }

    // And for an F12 screenshot's writer. The PNG encode of a double-wide capture runs off-thread
    // precisely because it is too slow to do inline, so the same unjoined-thread hazard applies --
    // and unlike the profiler's, this one is not behind a feature.
    if !screenshot::shutdown() {
        module::pin();
    }

    // The cleanups cleared render-thread-driven config flags (e.g. the HUD redirect). Give the still-
    // live hooks a few frames to tick those changes through -- the per-frame restore runs on the
    // render thread -- before uninstalling.
    std::thread::sleep(std::time::Duration::from_millis(100));

    tracing::info!("Uninstalling hooks");
    hooks::uninstall();

    // Wait to ensure we're clear of the blast radius of the hooks
    std::thread::sleep(std::time::Duration::from_millis(100));

    // The VEH registration must not outlive the DLL: ntdll would keep calling into the unmapped
    // image on the game's next routine first-chance exception.
    crash::uninstall();

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
    config::CONFIG.lock().far_field.gated_types = config::DEFAULT_FAR_FIELD_GATED_TYPES.to_owned();
    hud::install();
    capture::install();
    vr::install();
    tracing::info!("Initialized in game thread");

    Ok(())
}

/// Called to undo `initialize_from_game`; called once shutdown is triggered
fn shutdown_from_game() {
    // Re-create the original vertex shaders before anything is torn down: single-pass substitutes
    // patched (cb13-reading) shaders that the game holds, so without this the clean game would keep
    // rendering with them after eject. `SHUTTING_DOWN` is already set, so the reload's create hooks
    // are inert and produce the unpatched originals. A no-op unless single-pass patched any shaders.
    hooks::graphics_engine::shader::restore_original_shaders_on_eject();

    // Revert the far-field type gates and release the share pipeline's COM objects before the
    // hooks (and their patches) are torn down: the gated IsEnabled slots and the composite
    // pipeline must never outlive the payload code they point into.
    far_field::sync_type_gates("");
    far_field::share::teardown();
    // Unconditionally, not only when the debug UI came up: the cleanups undo engine-wide state --
    // the double-wide render resolution, the HUD redirect -- that a session sets whether or not egui
    // was ever shown.
    lifecycle::run_cleanups(
        EguiState::get()
            .as_mut()
            .map(|state| &mut state.egui_renderer),
    );
    EguiState::uninstall();
}

/// Whether shutdown/eject has begun. Read by subsystems that must quiesce the moment F5 is pressed --
/// notably the VR frame loop: after [`shutdown_from_game`] tears the OpenXR runtime down (persisting
/// the instance/session handles for the next injection), the game thread keeps ticking, and an
/// ungated `vr::update` re-acquires those persisted handles and rebuilds the swapchain, racing the
/// hook uninstall in [`shutdown_startup`] -- the crash on uninject.
pub(crate) fn is_shutting_down() -> bool {
    SHUTTING_DOWN.load(Ordering::Relaxed)
}

static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

/// When this payload was compiled, embedded by `build.rs`. Announced at startup (log, in-headset
/// banner, and the debug window): an uninject can leave the module resident (a hung shutdown
/// thread keeps the DLL mapped), and a failed re-inject then silently reactivates the stale code —
/// the stamp is how a stale payload is caught at a glance.
pub const BUILD_STAMP: &str = env!("JC3VRS_BUILD_STAMP");

/// Request that we shut down and exit
fn shutdown() {
    if SHUTTING_DOWN.swap(true, Ordering::SeqCst) {
        return;
    }

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
        // Close the previous real frame's puffin scopes and open the next; the frame scope itself
        // is opened by `game_update` right after this returns. Once per real frame, main thread.
        #[cfg(feature = "profiler")]
        profiler::new_frame();

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

            // Drive the egui UI as the VR floating panel when a session is running and the panel is
            // enabled (issue #24): size the layout to the panel texture and re-source the pointer from
            // the desktop mouse onto the panel surface. Off, this is `None` and the flat overlay path
            // is unchanged.
            let panel_size = crate::hud::egui_panel::active_size();
            egui_state.set_panel_mode(panel_size);
            if let Some(size) = panel_size {
                egui_state.push_events(crate::hud::pointer::window_mouse_events(size));
            }

            egui_state.run(|ctx, renderer| {
                // The per-eye capture textures feed the VR blit, the desktop mirror, and the F10
                // capture composite — they must be (re)created every frame, independent of which
                // UI tabs or windows are visible (or whether the Debug window is collapsed).
                ui::render::EGUI_DEBUG_RENDER_STATE
                    .lock()
                    .prepare_if_necessary(renderer);
                ui::startup_banner(ctx);
                // The dock area fills whatever space the window gives it, so the window carries an
                // explicit default size instead of auto-sizing to content.
                egui::Window::new("Debug")
                    .default_pos(egui::pos2(0.0, 0.0))
                    .default_size(egui::vec2(760.0, 560.0))
                    .resizable(true)
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
