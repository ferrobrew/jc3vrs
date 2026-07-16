use std::sync::OnceLock;

use jc3gi::game::GameState;
use parking_lot::{Mutex, MutexGuard};
use re_utilities::{ThreadSuspender, hook_library::HookLibrary};

pub mod animation;
pub mod camera;
pub mod character;
pub mod clock;
pub mod draw_count;
pub mod game;
pub mod graphics_engine;
pub mod input;
pub mod ui;
pub mod wndproc;

/// Whether the game is in interactive gameplay (`GameState::E_GAME_RUN`), as opposed to any
/// non-gameplay state (install, init, frontend, loading, startup). The single gameplay-boundary
/// predicate the hooks, the HUD, and the VR runtime gate on, so "are we in gameplay" is defined in
/// exactly one place and every consumer flips together.
pub fn in_gameplay() -> bool {
    // SAFETY: reads the process-global game-state word.
    unsafe { GameState::get() == GameState::E_GAME_RUN }
}

static HOOK_STATE: OnceLock<HookState> = OnceLock::new();
struct HookState {
    patcher: Mutex<re_utilities::Patcher>,
    hook_library: HookLibrary,
}
unsafe impl Send for HookState {}
unsafe impl Sync for HookState {}

pub(super) fn install() {
    let mut patcher = re_utilities::Patcher::new();
    let hook_library = ThreadSuspender::for_block(|| {
        Ok(HookLibrary::new()
            .with_hook_library(game::hook_library())
            .with_hook_library(clock::hook_library())
            .with_hook_library(graphics_engine::hook_library())
            .with_hook_library(draw_count::hook_library())
            .with_hook_library(camera::hook_library())
            .with_hook_library(wndproc::hook_library())
            .with_hook_library(character::hook_library())
            .with_hook_library(animation::hook_library())
            .with_hook_library(input::hook_library())
            .with_hook_library(ui::hook_library())
            .enable(&mut patcher)?)
    });
    let hook_library = match hook_library {
        Ok(hook_library) => hook_library,
        Err(e) => {
            tracing::error!("Failed to enable the hook library: {e:?}");
            return;
        }
    };
    let _ = HOOK_STATE.set(HookState {
        patcher: Mutex::new(patcher),
        hook_library,
    });
}

pub(super) fn uninstall() {
    let state = HOOK_STATE.get().unwrap();
    let _ = ThreadSuspender::for_block(|| {
        Ok(state
            .hook_library
            .set_enabled(&mut state.patcher.lock(), false)?)
    });
}

pub(super) fn patcher() -> Option<MutexGuard<'static, re_utilities::Patcher>> {
    HOOK_STATE.get().map(|state| state.patcher.lock())
}
