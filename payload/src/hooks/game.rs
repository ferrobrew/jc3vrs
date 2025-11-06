use detours_macro::detour;
use jc3gi::{
    clock::Clock,
    game::{Game, GameState, UpdateContexts},
};
use re_utilities::hook_library::HookLibrary;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&GAME_UPDATE_BINDER)
        // CGame::UpdateRender: nop out the check to m_UpdateFlags, we will _always_ be UpdateRender-ing.
        .with_patch(0x143_C7B_E0C, &[0x90].repeat(0x143_C7B_E25 - 0x143_C7B_E0C))
        .with_static_binder(&GAME_UPDATE_RENDER_BINDER)
        // CGame::UpdateRender: nop out everything between UpdateRender and ++this->m_RenderCount;
        // we'll be doing that ourselves!
        .with_patch(0x143_C7B_E3E, &[0x90].repeat(0x143_C7B_E78 - 0x143_C7B_E3E))
}

#[detour(address = 0x143_C7B_6A0)]
fn game_update(game: *const Game) -> bool {
    crate::update();
    GAME_UPDATE.get().unwrap().call(game)
}

#[detour(address = 0x143_C74_A90)]
fn game_update_render(game: *mut Game, update_contexts: *mut UpdateContexts) {
    unsafe {
        let spf = Clock::get().unwrap().GetSPF(false).min(0.5);

        GAME_UPDATE_RENDER
            .get()
            .unwrap()
            .call(game, update_contexts);
        GameState::PostUpdateRender(update_contexts);
        game.as_mut().unwrap().Draw(spf);
    }
}
