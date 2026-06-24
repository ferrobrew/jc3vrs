use std::sync::OnceLock;

use parking_lot::{Mutex, MutexGuard};
use re_utilities::{ThreadSuspender, hook_library::HookLibraries};

pub mod camera;
pub mod character;
pub mod clock;
pub mod draw_count;
pub mod game;
pub mod graphics_engine;
pub mod ui;
pub mod wndproc;

static HOOK_STATE: OnceLock<HookState> = OnceLock::new();
struct HookState {
    patcher: Mutex<re_utilities::Patcher>,
    hook_libraries: HookLibraries,
}
unsafe impl Send for HookState {}
unsafe impl Sync for HookState {}

pub(super) fn install() {
    let mut patcher = re_utilities::Patcher::new();
    let hook_libraries = ThreadSuspender::for_block(|| {
        Ok(HookLibraries::new([
            game::hook_library(),
            clock::hook_library(),
            graphics_engine::hook_library(),
            draw_count::hook_library(),
            camera::hook_library(),
            wndproc::hook_library(),
            character::hook_library(),
            ui::hook_library(),
        ])
        .enable(&mut patcher)?)
    });
    let hook_libraries = match hook_libraries {
        Ok(hook_libraries) => hook_libraries,
        Err(e) => {
            tracing::error!("Failed to enable hook libraries: {e:?}");
            return;
        }
    };
    let _ = HOOK_STATE.set(HookState {
        patcher: Mutex::new(patcher),
        hook_libraries,
    });
}

pub(super) fn uninstall() {
    let hook_libraries = HOOK_STATE.get().unwrap();
    let _ = ThreadSuspender::for_block(|| {
        Ok(hook_libraries
            .hook_libraries
            .set_enabled(&mut hook_libraries.patcher.lock(), false)?)
    });
}

pub(super) fn patcher() -> Option<MutexGuard<'static, re_utilities::Patcher>> {
    HOOK_STATE.get().map(|state| state.patcher.lock())
}
