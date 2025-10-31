use detours_macro::detour;
use jc3gi::graphics_engine::{
    device::{Context, Device},
    graphics_engine::GraphicsEngine,
    render_engine::RenderEngine,
};
use re_utilities::hook_library::HookLibrary;
use windows::Win32::System::Threading::{EnterCriticalSection, LeaveCriticalSection};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&GRAPHICS_FLIP_BINDER)
        .with_static_binder(&RENDER_ENGINE_POST_DRAW_BINDER)
}

#[detour(address = 0x145_34B_870)]
fn graphics_flip(device: *mut Device) -> u64 {
    if let Some(egui_state) = crate::egui_impl::EguiState::get().as_mut() {
        egui_state.render();
    }
    GRAPHICS_FLIP.get().unwrap().call(device)
}

#[detour(address = 0x143_409_1A0)]
fn render_engine_post_draw(render_engine: *mut RenderEngine, context: *mut Context) -> u64 {
    let result = RENDER_ENGINE_POST_DRAW
        .get()
        .unwrap()
        .call(render_engine, context);

    unsafe {
        let Some(context) = context.as_mut() else {
            return result;
        };

        let Some(graphics_engine) = GraphicsEngine::get() else {
            return result;
        };

        let Some(backbuffer_linear) = graphics_engine.m_BackBufferLinear.as_ref() else {
            return result;
        };

        let lock = crate::EGUI_DEBUG_RENDER_STATE.lock();
        let Some(texture) = lock.texture() else {
            return result;
        };

        EnterCriticalSection(context.m_Mutex);

        context
            .m_Context
            .CopyResource(texture, &backbuffer_linear.m_Texture);

        LeaveCriticalSection(context.m_Mutex);
    }

    result
}
