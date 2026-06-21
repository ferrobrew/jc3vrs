use std::sync::atomic::{AtomicBool, Ordering};

use detours_macro::detour;
use jc3gi::clock::Clock;
use re_utilities::hook_library::HookLibrary;

/// Reset at the start of each real frame (in `game_update`). While stereo is active, the
/// `CClock::Update` detour gates the clock to once per real frame, so a second `Draw` doesn't run
/// the SPF exponential smoother twice and drag the game into slow motion.
pub static UPDATED_THIS_FRAME: AtomicBool = AtomicBool::new(false);

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&CLOCK_UPDATE_BINDER)
}

#[detour(address = jc3gi::clock::Clock::Update_ADDRESS)]
fn clock_update(clock: *mut Clock) {
    if crate::stereo::active() && UPDATED_THIS_FRAME.swap(true, Ordering::Relaxed) {
        return;
    }
    CLOCK_UPDATE.get().unwrap().call(clock);
}
