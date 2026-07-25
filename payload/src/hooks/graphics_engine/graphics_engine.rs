use std::sync::atomic::AtomicBool;

use detours_macro::detour;
use jc3gi::{
    graphics_engine::{
        device::{Context, Device, DeviceInfo},
        graphics_engine::{GraphicsEngine, HDevice_t},
        render_engine::RenderEngine,
    },
    ui::ui_manager::GetIUIManager,
};
use re_utilities::hook_library::HookLibrary;
use windows::Win32::{
    Graphics::Direct3D11::D3D11_BOX,
    System::Threading::{EnterCriticalSection, LeaveCriticalSection},
};

use crate::debug::trace::{TraceEvent, TraceState};

pub(super) fn hook_library() -> HookLibrary {
    HookLibrary::new()
        .with_static_binder(&GRAPHICS_FLIP_BINDER)
        .with_static_binder(&GRAPHICS_ENGINE_DRAW_BINDER)
        .with_static_binder(&RENDER_ENGINE_POST_DRAW_BINDER)
        .with_static_binder(&CREATE_RENDER_SETUPS_BINDER)
        .with_static_binder(&DEVICE_RESIZE_BUFFERS_BINDER)
}

/// Keep the DXGI swapchain at the window size while the engine resizes everything else.
///
/// While the mod owns the back buffer, this is a **substitute** for the device-level
/// `ResizeBuffers` rather than a suppression of it: `ApplyResize` reads the new size back out of
/// `device->m_DeviceInfo` immediately afterwards and feeds it to `CreateRenderSetups` and to every
/// registered resize callback, so a plain no-op would leave the whole pipeline at the old size.
/// Writing the device info without touching DXGI lets `ApplyResize` run verbatim -- scene targets,
/// pass pools, UI reset, and camera aspect all follow the render size -- while the swapchain stays
/// where it is. See `docs/mod/swapchain-ownership.md` §5.1.
///
/// `device->m_BackBuffer`'s own dimensions are written only by the real function, so under the
/// substitute they keep reporting the true swapchain size: after this, `m_BackBuffer` means "the
/// window" and `m_BackBufferLinear` means "the render target".
#[detour(address = jc3gi::graphics_engine::device::ResizeBuffers_ADDRESS)]
fn device_resize_buffers(device: *mut HDevice_t, width: u32, height: u32) -> bool {
    // The mod drives a real resize of its own once the substitution is installed, to bring an
    // already-oversized swapchain down to the window; stand aside for that one.
    if !crate::vr::back_buffer_owned() || crate::vr::resize_substitute_bypassed() {
        return DEVICE_RESIZE_BUFFERS
            .get()
            .unwrap()
            .call(device, width, height);
    }
    // SAFETY: the engine passes its live device; `ApplyResize` runs on the drained idle context.
    let Some(dev) = (unsafe { device.cast::<Device>().as_mut() }) else {
        return DEVICE_RESIZE_BUFFERS
            .get()
            .unwrap()
            .call(device, width, height);
    };
    dev.m_DeviceInfo.m_DisplayWidth = width;
    dev.m_DeviceInfo.m_DisplayHeight = height;
    dev.m_DeviceInfo.m_DisplayRatio = if height > 0 {
        width as f32 / height as f32
    } else {
        0.0
    };
    // Deliberately claims a resize that did not happen to the swapchain. `m_WasResized` is the
    // engine's own "the buffers moved since you last looked" flag, and setting it is what keeps the
    // rest of `ApplyResize` -- the resize callbacks, the pass-owned pools -- behaving exactly as they
    // do on a real resize, which is the point of substituting rather than suppressing. The render
    // targets really did change size; only the DXGI buffers did not.
    dev.m_WasResized = true;
    true
}

/// The back-buffer substitution point: every path that rebuilds the engine's swapchain-derived render
/// setups funnels through `CreateRenderSetups` (its only callers are `InitializeSystem` and
/// `ApplyResize`), so an epilogue here cannot be missed. Inert unless the mod owns the back buffer;
/// see [`crate::vr::back_buffer`] and `docs/mod/swapchain-ownership.md`.
#[detour(
    address = jc3gi::graphics_engine::graphics_engine::GraphicsEngine::CreateRenderSetups_ADDRESS
)]
fn create_render_setups(this: *mut GraphicsEngine, device_info: *const DeviceInfo) -> bool {
    let returned = CREATE_RENDER_SETUPS.get().unwrap().call(this, device_info);
    // Deliberately *not* gated on `returned`: the release build never sets a return value. Its C++
    // signature says `bool` and the symbol-dump build ends in `return 1`, but release codegen dropped
    // that -- the function's last instruction before the epilogue is the `CreateRenderSetup` call, so
    // `al` is the low byte of an aligned pointer. Read as a `bool` that is reliably even, and a
    // bit-0 test on it is therefore always false. Gating on it silently disabled the substitution.
    //
    // SAFETY: the engine has just rebuilt its own render setups, on the drained idle context that
    // `CreateRenderSetups` runs under (the `Draw` prologue).
    if let Some(engine) = unsafe { this.as_mut() } {
        unsafe { crate::vr::substitute_render_setups(engine) };
    }
    returned
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
        if crate::hooks::in_gameplay()
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
    // The last render seam of the dispatch: bracket PostDraw on both timelines, then close the GPU
    // dispatch opened in `render_pass::pre_draw` (ending the disjoint query and reading back the
    // dispatches the GPU has since finished). `Context` and `HContext_t` are the same handle.
    #[cfg(feature = "profiler")]
    puffin::profile_scope!("RenderEngine::PostDraw");
    let result = {
        #[cfg(feature = "profiler")]
        // SAFETY: `context` is the live immediate-context handle for this dispatch.
        let _gpu = unsafe {
            crate::profiler::gpu::seam(context.cast(), crate::profiler::gpu::GpuSeam::PostDraw)
        };
        RENDER_ENGINE_POST_DRAW
            .get()
            .unwrap()
            .call(render_engine, context)
    };
    #[cfg(feature = "profiler")]
    // SAFETY: `context` is the live immediate-context handle for this dispatch.
    unsafe {
        crate::profiler::gpu::end_dispatch(context.cast())
    };
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
            // The interactive egui debug panel (issue #24) is an independent floating surface drawn
            // right after the gameplay HUD. Under collapse the render camera is centered and the
            // target is double-wide, so draw both world-locked overlays once per eye into each half
            // with that eye's own VP (see `single_pass::collapse_ui_eye_override`); otherwise the
            // per-dispatch single draw carries the eye implicitly.
            if crate::stereo::single_pass::collapse_active() {
                for eye in 0..2 {
                    crate::stereo::single_pass::set_collapse_ui_eye(Some(eye));
                    crate::hud::draw_quad(&context.m_Context, device, back_buffer, eye);
                    crate::hud::egui_panel::draw_quad(&context.m_Context, device, back_buffer, eye);
                }
                crate::stereo::single_pass::set_collapse_ui_eye(None);
            } else {
                crate::hud::draw_quad(&context.m_Context, device, back_buffer, index);
                crate::hud::egui_panel::draw_quad(&context.m_Context, device, back_buffer, index);
            }
            // Redirect the flat mirror overlay into an offscreen texture on eye 0 (consuming this
            // frame's egui output) so the desktop mirror can composite it from the deferred frame
            // tail's thread. A no-op unless a session renders, the mirror is on, and the panel is
            // off. See `crate::hud::mirror_overlay`.
            if index == 0 {
                crate::hud::mirror_overlay::render(&context.m_Context, device, back_buffer);
            }
        }

        // Final back buffer for this eye. (The HDR scene / MainColor is captured earlier, at the
        // start of the post chain, before it gets read and recycled -- see capture_main_color.)
        if let Some(src) = graphics_engine.m_BackBufferLinear.as_ref() {
            if crate::stereo::single_pass::collapse_active() {
                // The collapsed single walk rendered both eyes into this one (viewport-split) back
                // buffer -- left half eye 0, right half eye 1 -- so copy each half into its eye
                // texture. `half_w` is a per-eye-width region: with `single_pass_double_wide` the back
                // buffer is 2x per-eye wide, so each half fills its full-res eye texture; without it
                // each half is squished and fills only the left portion of the eye texture (a
                // bring-up limitation, not the finished look).
                let half_w = u32::from(src.m_Width) / 2;
                let height = u32::from(src.m_Height);
                for eye in 0..2u32 {
                    if let Some(dst) = lock.texture(eye as usize) {
                        let region = D3D11_BOX {
                            left: eye * half_w,
                            top: 0,
                            front: 0,
                            right: (eye + 1) * half_w,
                            bottom: height,
                            back: 1,
                        };
                        context.m_Context.CopySubresourceRegion(
                            dst,
                            0,
                            0,
                            0,
                            0,
                            &src.m_Texture,
                            0,
                            Some(&region),
                        );
                    }
                }
            } else if let Some(dst) = lock.texture(index) {
                context.m_Context.CopyResource(dst, &src.m_Texture);
            }

            // Service an F12 screenshot request: the linear back buffer is this frame's final render
            // (under collapse, both eye-halves side by side). A no-op unless one was requested.
            if let Some(device) = graphics_engine.m_Device.as_ref() {
                crate::screenshot::capture_if_requested(
                    &device.m_Device,
                    &context.m_Context,
                    &src.m_Texture,
                );
            }
        }

        LeaveCriticalSection(context.m_Mutex);
    }

    result
}
