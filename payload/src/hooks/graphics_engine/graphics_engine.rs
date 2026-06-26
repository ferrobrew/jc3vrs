use std::sync::atomic::AtomicBool;

use detours_macro::detour;
use jc3gi::{
    game::GameState,
    graphics_engine::{
        device::{Context, Device},
        graphics_engine::GraphicsEngine,
        render_engine::RenderEngine,
    },
    ui::ui_manager::GetIUIManager,
};
use re_utilities::hook_library::HookLibrary;
use windows::Win32::System::Threading::{EnterCriticalSection, LeaveCriticalSection};

use crate::debug::trace::{TraceEvent, TraceState};

pub(super) fn extend(library: HookLibrary) -> HookLibrary {
    library
        .with_static_binder(&GRAPHICS_FLIP_BINDER)
        .with_static_binder(&GRAPHICS_ENGINE_DRAW_BINDER)
        .with_static_binder(&RENDER_ENGINE_POST_DRAW_BINDER)
}

// `CGame::Draw` clears `m_DrawScene` while a static-background full-screen UI is up (pause / map), so
// the draw thread renders only the UI and clears the eye to transparent -- a black void behind the
// floating panel in VR. Force the 3D scene to keep rendering during gameplay menus so the
// frozen-but-head-trackable world stays visible behind the panel. Gated to E_GAME_RUN + a static
// background so loading screens, the frontend, and full-screen videos are untouched. See issue #7.
#[detour(address = jc3gi::graphics_engine::graphics_engine::GraphicsEngine::Draw_ADDRESS)]
fn graphics_engine_draw(graphics_engine: *mut GraphicsEngine, dt: f32) {
    // SAFETY: runs inside `CGame::Draw` (which just set the flag) before the draw is dispatched, on
    // the render thread; `graphics_engine` is the live engine and `GetIUIManager` the live UI.
    unsafe {
        if GameState::get() == GameState::E_GAME_RUN
            && let Some(ge) = graphics_engine.as_mut()
            && let Some(ui) = GetIUIManager().as_ref()
            && ui.IsUsingStaticBackGround()
        {
            ge.m_DrawScene = true;
        }
        GRAPHICS_ENGINE_DRAW
            .get()
            .unwrap()
            .call(graphics_engine, dt);
    }
}

pub static BLOCK_FLIP: AtomicBool = AtomicBool::new(false);

#[detour(address = jc3gi::graphics_engine::graphics_engine::graphics_flip_ADDRESS)]
fn graphics_flip(device: *mut Device) -> u64 {
    let blocked = BLOCK_FLIP.load(std::sync::atomic::Ordering::Relaxed);
    TraceState::record_eye(TraceEvent::Flip { blocked });
    tracing::trace!(target: "frameloop", "graphics_flip: entry (blocked={blocked})");
    if blocked {
        tracing::trace!(target: "frameloop", "graphics_flip: blocked, returning");
        return 0;
    }

    if let Some(egui_state) = crate::egui_impl::EguiState::get().as_mut() {
        // Hide the debug overlay while the F10 capture window is up, so the recording is clean.
        if !crate::capture::is_active() {
            egui_state.render();
        }
    }
    tracing::trace!(target: "frameloop", "graphics_flip: calling original");
    let r = GRAPHICS_FLIP.get().unwrap().call(device);
    tracing::trace!(target: "frameloop", "graphics_flip: original returned");
    r
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

        // Draw the floating HUD quad onto this eye's back buffer before it is captured/presented, so it
        // shows in both the preview and the final image. The HUD render target is also cleared so the
        // next frame starts clean rather than accumulating past frames.
        if let (Some(device), Some(back_buffer)) = (
            graphics_engine.m_Device.as_ref(),
            graphics_engine.m_BackBufferLinear.as_ref(),
        ) {
            crate::hud::draw_quad(&context.m_Context, device, back_buffer, index);
        }

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
