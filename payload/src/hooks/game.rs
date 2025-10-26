use detours_macro::detour;
use jc3gi::game::Game;
use re_utilities::hook_library::HookLibrary;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&GAME_UPDATE_BINDER)
}

#[detour(address = 0x143_C7B_6A0)]
fn game_update(game: *const Game) -> bool {
    crate::update();
    GAME_UPDATE.get().unwrap().call(game)
}
