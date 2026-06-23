use std::sync::atomic::AtomicBool;

use detours_macro::detour;
use jc3gi::graphics_engine::{
    device::{Context, Device},
    graphics_engine::GraphicsEngine,
    render_engine::RenderEngine,
};
use re_utilities::hook_library::HookLibrary;
use windows::Win32::System::Threading::{EnterCriticalSection, LeaveCriticalSection};

use crate::debug::trace::{TraceEvent, TraceState};

pub(super) fn extend(library: HookLibrary) -> HookLibrary {
    library
        .with_static_binder(&GRAPHICS_FLIP_BINDER)
        .with_static_binder(&RENDER_ENGINE_POST_DRAW_BINDER)
}

pub static BLOCK_FLIP: AtomicBool = AtomicBool::new(false);

#[detour(address = jc3gi::graphics_engine::graphics_engine::graphics_flip_ADDRESS)]
fn graphics_flip(device: *mut Device) -> u64 {
    let blocked = BLOCK_FLIP.load(std::sync::atomic::Ordering::Relaxed);
    TraceState::record_eye(TraceEvent::Flip { blocked });
    if blocked {
        return 0;
    }

    if let Some(egui_state) = crate::egui_impl::EguiState::get().as_mut() {
        egui_state.render();
    }
    GRAPHICS_FLIP.get().unwrap().call(device)
}

#[detour(address = jc3gi::graphics_engine::render_engine::RenderEngine::PostDraw_ADDRESS)]
fn render_engine_post_draw(render_engine: *mut RenderEngine, context: *mut Context) -> u64 {
    let result = RENDER_ENGINE_POST_DRAW
        .get()
        .unwrap()
        .call(render_engine, context);
    TraceState::record_eye(TraceEvent::PostDraw);

    unsafe {
        let Some(context) = context.as_mut() else {
            return result;
        };

        let Some(graphics_engine) = GraphicsEngine::get() else {
            return result;
        };

        // Drive the HUD redirect on the render thread: redirect while enabled, restore while disabled.
        // The rebind is sticky, so applying it here -- before the UI renders later in the frame --
        // takes effect on the next UI render.
        if let Some(device) = graphics_engine.m_Device.as_ref()
            && let Some(back_buffer) = device.m_BackBuffer.as_ref()
        {
            crate::hud::tick(
                device,
                u32::from(back_buffer.m_Width),
                u32::from(back_buffer.m_Height),
            );
        }

        let lock = crate::ui::render::EGUI_DEBUG_RENDER_STATE.lock();
        let index = crate::stereo::draw_index();

        EnterCriticalSection(context.m_Mutex);

        // Final back buffer for this eye. (The HDR scene / MainColor is captured earlier, at the
        // start of the post chain, before it gets read and recycled -- see capture_main_color.)
        if let (Some(dst), Some(src)) = (
            lock.texture(index),
            graphics_engine.m_BackBufferLinear.as_ref(),
        ) {
            context.m_Context.CopyResource(dst, &src.m_Texture);
        }

        LeaveCriticalSection(context.m_Mutex);
    }

    result
}
