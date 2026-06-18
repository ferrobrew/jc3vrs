use detours_macro::detour;
use jc3gi::{
    clock::Clock,
    game::{Game, GameState, UpdateContexts},
};
use re_utilities::hook_library::HookLibrary;

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&GAME_UPDATE_BINDER)
        // CGame::Update + 0x787: nop the `m_UpdateFlags & 4` check (the jz), we will _always_ be UpdateRender-ing.
        .with_patch(Game::Update_ADDRESS + 0x787, &[0x90; 2])
        .with_static_binder(&GAME_UPDATE_RENDER_BINDER)
        // CGame::Update + 0x7A2: nop everything between UpdateRender and ++this->m_RenderCount;
        // we'll be doing that ourselves!
        .with_patch(Game::Update_ADDRESS + 0x7A2, &[0x90; 0x3A])
}

#[detour(address = jc3gi::game::Game::Update_ADDRESS)]
fn game_update(game: *const Game) -> bool {
    crate::update();
    GAME_UPDATE.get().unwrap().call(game)
}

#[detour(address = jc3gi::game::Game::UpdateRender_ADDRESS)]
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
