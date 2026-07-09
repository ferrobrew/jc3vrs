use anyhow::Context as _;
use jc3gi::graphics_engine::graphics_engine::{ActiveCursor, GraphicsEngine, get_graphics_params};
use parking_lot::{Mutex, MutexGuard};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    Graphics::Direct3D11::{ID3D11DeviceContext, ID3D11RenderTargetView},
};

pub struct EguiState {
    start_time: std::time::Instant,
    egui_context: egui::Context,
    pub egui_renderer: egui_directx11::Renderer,
    renderer_output: Option<egui_directx11::RendererOutput>,
    game_input_state: Option<GameInputState>,
    events: Vec<egui::Event>,
    /// When `Some`, the egui UI is being driven as the VR floating panel (issue #24): [`run`] sizes
    /// the layout to this panel-texture size instead of the game window, and [`render_to`] paints it
    /// into the panel render target. Set from the game thread each frame before [`run`]; `None` is
    /// the flat back-buffer overlay (the default, unchanged) behavior.
    panel: Option<(u32, u32)>,
}
struct GameInputState {
    input_was_enabled: bool,
    active_cursor: ActiveCursor,
}
static STATE: Mutex<Option<EguiState>> = Mutex::new(None);
impl EguiState {
    fn new() -> anyhow::Result<Self> {
        let device = unsafe {
            &GraphicsEngine::get()
                .context("Failed to get graphics engine")?
                .m_Device
                .as_mut()
                .context("Failed to get device")?
                .m_Device
        };
        Ok(Self {
            start_time: std::time::Instant::now(),
            egui_context: egui::Context::default(),
            egui_renderer: egui_directx11::Renderer::new(device)
                .context("Failed to create egui renderer")?,
            renderer_output: None,
            game_input_state: None,
            events: Vec::new(),
            panel: None,
        })
    }

    pub fn install() -> anyhow::Result<()> {
        STATE.lock().replace(Self::new()?);
        tracing::info!("Initialized egui");
        Ok(())
    }

    pub fn uninstall() {
        if let Some(mut state) = STATE.lock().take()
            && state.game_input_state.is_some()
        {
            state.toggle_game_input_capture();
        }
        tracing::info!("Uninitialized egui");
    }

    pub fn get() -> MutexGuard<'static, Option<Self>> {
        STATE.lock()
    }

    /// Select the panel (VR) or flat (desktop) layout mode for the next [`run`]. `Some(size)` sizes
    /// the egui layout to a panel texture of that size (VR floating panel, issue #24); `None` sizes it
    /// to the game window (the flat back-buffer overlay). Call from the game thread before [`run`].
    pub fn set_panel_mode(&mut self, size: Option<(u32, u32)>) {
        self.panel = size;
    }

    /// Append externally-sourced input events (the VR panel's virtual pointer, issue #24) to the next
    /// [`run`]'s event queue. Call from the game thread before [`run`].
    pub fn push_events(&mut self, events: impl IntoIterator<Item = egui::Event>) {
        self.events.extend(events);
    }

    pub fn run(&mut self, mut callback: impl FnMut(&egui::Context, &mut egui_directx11::Renderer)) {
        let screen_size = match self.panel {
            Some((width, height)) => (width as f32, height as f32),
            None => {
                let params = unsafe { get_graphics_params() };
                (params.m_Width as f32, params.m_Height as f32)
            }
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_max(
                Default::default(),
                egui::Pos2::new(screen_size.0, screen_size.1),
            )),
            time: Some(self.start_time.elapsed().as_secs_f64()),
            focused: self.game_input_state.is_some(),
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };
        let egui_output = self
            .egui_context
            .run(input, |ctx| callback(ctx, &mut self.egui_renderer));
        let (mut renderer_output, platform_output, _) = egui_directx11::split_output(egui_output);

        // Carry a still-unrendered frame's texture delta forward instead of dropping it. `run()` (game
        // thread) can outpace `render_to()` (render thread) and overwrite an output that was never
        // painted; egui emits the font atlas `set` only on the frame it (re)builds it, so a dropped
        // frame silently loses the atlas upload and every later frame samples a non-existent
        // `Managed(0)`, blanking all text. Appending the older delta first preserves those uploads (the
        // standard egui skipped-frame handling); the newer geometry still replaces the older.
        if let Some(prev) = self.renderer_output.take() {
            let mut delta = prev.textures_delta;
            delta.append(renderer_output.textures_delta);
            renderer_output.textures_delta = delta;
        }

        if self.is_input_captured()
            && let Some(graphics_engine) = unsafe { GraphicsEngine::get() }
        {
            use egui::CursorIcon as CI;
            graphics_engine.m_ActiveCursor = match platform_output.cursor_icon {
                CI::None => ActiveCursor::None,
                CI::AllScroll => ActiveCursor::Cross,
                CI::ResizeHorizontal | CI::ResizeEast | CI::ResizeWest => ActiveCursor::Slider,
                CI::ZoomIn | CI::ZoomOut => ActiveCursor::Zoom,
                _ => ActiveCursor::Arrow,
            };
        }
        self.renderer_output = Some(renderer_output);
    }

    pub fn render(&mut self) {
        let Some(renderer_output) = self.renderer_output.take() else {
            return;
        };
        unsafe {
            let Some(device) = GraphicsEngine::get().and_then(|ge| ge.m_Device.as_mut()) else {
                return;
            };
            let Some(context) = device.m_Context.as_mut() else {
                return;
            };
            let Some(backbuffer) = device.m_BackBuffer.as_mut() else {
                return;
            };
            if let Err(e) = self.egui_renderer.render(
                &context.m_Context,
                &backbuffer.m_RTV,
                &self.egui_context,
                renderer_output,
            ) {
                tracing::error!("Failed to render egui: {e:?}");
            }
        }
    }

    /// Render the current frame's egui output into an arbitrary render target (the VR floating panel's
    /// offscreen texture, issue #24) using the supplied context, consuming the single-use
    /// [`renderer_output`](EguiState::renderer_output). The target is cleared to transparent first: the
    /// egui renderer does not clear and early-returns on an empty frame, so without the clear the panel
    /// would retain the previous frame. A no-op when the output has already been consumed this frame.
    /// The caller must hold the engine context mutex.
    pub fn render_to(&mut self, context: &ID3D11DeviceContext, rtv: &ID3D11RenderTargetView) {
        // SAFETY: `rtv` is a live render-target view; `context` is the caller-locked engine context.
        unsafe {
            context.ClearRenderTargetView(rtv, &[0.0, 0.0, 0.0, 0.0]);
        }
        let Some(renderer_output) = self.renderer_output.take() else {
            return;
        };
        if let Err(e) = self
            .egui_renderer
            .render(context, rtv, &self.egui_context, renderer_output)
        {
            tracing::error!("Failed to render egui panel: {e:?}");
        }
    }

    pub fn toggle_game_input_capture(&mut self) {
        unsafe {
            let Some(input_device_manager) =
                jc3gi::input::input_device_manager::InputDeviceManager::get()
            else {
                return;
            };
            let Some(graphics_engine) = GraphicsEngine::get() else {
                return;
            };

            self.events.clear();
            if let Some(game_input_state) = self.game_input_state.take() {
                input_device_manager.m_Enabled = game_input_state.input_was_enabled;
                graphics_engine.m_ActiveCursor = game_input_state.active_cursor;
            } else {
                self.game_input_state = Some(GameInputState {
                    input_was_enabled: input_device_manager.m_Enabled,
                    active_cursor: graphics_engine.m_ActiveCursor,
                });

                input_device_manager.m_Enabled = false;
            }
        }
    }

    pub fn is_input_captured(&self) -> bool {
        self.game_input_state.is_some()
    }

    pub fn wndproc(&mut self, _hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) {
        if !self.is_input_captured() {
            return;
        }
        // In VR-panel mode the pointer is re-sourced from the panel's virtual cursor (issue #24), so
        // the window-pixel mouse messages -- whose coordinates are in the game window's space, not the
        // panel texture's -- must not reach egui. Keyboard, text, and paste still flow unchanged.
        if self.panel.is_some() && is_mouse_message(msg) {
            return;
        }
        wndproc(&mut self.events, msg, wparam, lparam);
    }
}

/// Whether `msg` is a mouse move/button/wheel message whose coordinates are window-relative.
fn is_mouse_message(msg: u32) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
        WM_RBUTTONDOWN, WM_RBUTTONUP,
    };
    matches!(
        msg,
        WM_MOUSEMOVE
            | WM_LBUTTONDOWN
            | WM_LBUTTONUP
            | WM_RBUTTONDOWN
            | WM_RBUTTONUP
            | WM_MBUTTONDOWN
            | WM_MBUTTONUP
            | WM_MOUSEWHEEL
    )
}

pub fn wndproc(events: &mut Vec<egui::Event>, msg: u32, wparam: WPARAM, lparam: LPARAM) {
    use windows::Win32::UI::WindowsAndMessaging::*;

    let wparam = wparam.0;
    let lparam = lparam.0;

    match msg {
        WM_MOUSEMOVE => {
            let x = (lparam & 0xFFFF) as i16 as f32;
            let y = ((lparam >> 16) & 0xFFFF) as i16 as f32;
            events.push(egui::Event::PointerMoved(egui::Pos2::new(x, y)));
        }

        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
            let (x, y) = (
                (lparam & 0xFFFF) as i16 as f32,
                ((lparam >> 16) & 0xFFFF) as i16 as f32,
            );
            let button = match msg {
                WM_LBUTTONDOWN => egui::PointerButton::Primary,
                WM_RBUTTONDOWN => egui::PointerButton::Secondary,
                WM_MBUTTONDOWN => egui::PointerButton::Middle,
                _ => unreachable!(),
            };
            events.push(egui::Event::PointerButton {
                pos: egui::Pos2::new(x, y),
                button,
                pressed: true,
                modifiers: get_modifiers(),
            });
        }

        WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
            let (x, y) = (
                (lparam & 0xFFFF) as i16 as f32,
                ((lparam >> 16) & 0xFFFF) as i16 as f32,
            );
            let button = match msg {
                WM_LBUTTONUP => egui::PointerButton::Primary,
                WM_RBUTTONUP => egui::PointerButton::Secondary,
                WM_MBUTTONUP => egui::PointerButton::Middle,
                _ => unreachable!(),
            };
            events.push(egui::Event::PointerButton {
                pos: egui::Pos2::new(x, y),
                button,
                pressed: false,
                modifiers: get_modifiers(),
            });
        }

        WM_MOUSEWHEEL => {
            let delta = (wparam >> 16) as i16 as f32 / WHEEL_DELTA as f32;
            events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::Vec2::new(0.0, delta),
                modifiers: get_modifiers(),
            });
        }

        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if let Some(key) = virtual_key_to_egui_key(wparam as u32) {
                let modifiers = get_modifiers();

                // Special-case some key combinations, as is done by egui-winit
                // <https://github.com/emilk/egui/blob/9a1e358a144b5d2af9d03a80257c34883f57cf0b/crates/egui-winit/src/lib.rs#L754>
                if is_copy_command(modifiers, key) {
                    events.push(egui::Event::Copy);
                } else if is_cut_command(modifiers, key) {
                    events.push(egui::Event::Cut);
                } else if is_paste_command(modifiers, key) {
                    push_paste_command_if_available(events);
                } else {
                    events.push(egui::Event::Key {
                        key,
                        physical_key: None, // Windows doesn't provide this information easily
                        pressed: true,
                        repeat: (lparam & 0x40000000) != 0,
                        modifiers,
                    });
                }
            }
        }

        WM_KEYUP | WM_SYSKEYUP => {
            if let Some(key) = virtual_key_to_egui_key(wparam as u32) {
                events.push(egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: false,
                    repeat: false,
                    modifiers: get_modifiers(),
                });
            }
        }

        WM_CHAR => {
            let ch = wparam as u8 as char;
            if !ch.is_control() {
                events.push(egui::Event::Text(ch.to_string()));
            }
        }

        WM_PASTE => {
            push_paste_command_if_available(events);
        }

        _ => {}
    }
}

fn get_modifiers() -> egui::Modifiers {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
    unsafe {
        egui::Modifiers {
            alt: (GetKeyState(VK_MENU.0 as _) as u16 & 0x8000) != 0,
            ctrl: (GetKeyState(VK_CONTROL.0 as _) as u16 & 0x8000) != 0,
            shift: (GetKeyState(VK_SHIFT.0 as _) as u16 & 0x8000) != 0,
            mac_cmd: false,
            command: (GetKeyState(VK_CONTROL.0 as _) as u16 & 0x8000) != 0,
        }
    }
}

fn push_paste_command_if_available(events: &mut Vec<egui::Event>) {
    if let Ok(contents) = clipboard_win::get_clipboard_string() {
        let contents = contents.replace("\r\n", "\n");
        if !contents.is_empty() {
            events.push(egui::Event::Paste(contents));
        }
    }
}

// The below functions are copied from egui-winit:
// <https://github.com/emilk/egui/blob/9a1e358a144b5d2af9d03a80257c34883f57cf0b/crates/egui-winit/src/lib.rs#L1017-L1033>
fn is_cut_command(modifiers: egui::Modifiers, keycode: egui::Key) -> bool {
    keycode == egui::Key::Cut
        || (modifiers.command && keycode == egui::Key::X)
        || (cfg!(target_os = "windows") && modifiers.shift && keycode == egui::Key::Delete)
}

fn is_copy_command(modifiers: egui::Modifiers, keycode: egui::Key) -> bool {
    keycode == egui::Key::Copy
        || (modifiers.command && keycode == egui::Key::C)
        || (cfg!(target_os = "windows") && modifiers.ctrl && keycode == egui::Key::Insert)
}

fn is_paste_command(modifiers: egui::Modifiers, keycode: egui::Key) -> bool {
    keycode == egui::Key::Paste
        || (modifiers.command && keycode == egui::Key::V)
        || (cfg!(target_os = "windows") && modifiers.shift && keycode == egui::Key::Insert)
}

fn virtual_key_to_egui_key(vk: u32) -> Option<egui::Key> {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    match VIRTUAL_KEY(vk as _) {
        VK_DOWN => Some(egui::Key::ArrowDown),
        VK_LEFT => Some(egui::Key::ArrowLeft),
        VK_RIGHT => Some(egui::Key::ArrowRight),
        VK_UP => Some(egui::Key::ArrowUp),
        VK_ESCAPE => Some(egui::Key::Escape),
        VK_TAB => Some(egui::Key::Tab),
        VK_BACK => Some(egui::Key::Backspace),
        VK_RETURN => Some(egui::Key::Enter),
        VK_SPACE => Some(egui::Key::Space),
        VK_INSERT => Some(egui::Key::Insert),
        VK_DELETE => Some(egui::Key::Delete),
        VK_HOME => Some(egui::Key::Home),
        VK_END => Some(egui::Key::End),
        VK_PRIOR => Some(egui::Key::PageUp),
        VK_NEXT => Some(egui::Key::PageDown),
        VK_OEM_2 => Some(egui::Key::Slash),
        VK_OEM_5 => Some(egui::Key::Backslash),
        VK_OEM_PERIOD => Some(egui::Key::Period),
        VK_OEM_COMMA => Some(egui::Key::Comma),
        VK_OEM_7 => Some(egui::Key::Quote),
        VK_OEM_4 => Some(egui::Key::OpenBracket),
        VK_OEM_6 => Some(egui::Key::CloseBracket),
        VK_OEM_3 => Some(egui::Key::Backtick),
        VK_OEM_MINUS => Some(egui::Key::Minus),
        VK_OEM_PLUS => Some(egui::Key::Plus),
        VK_0 => Some(egui::Key::Num0),
        VK_1 => Some(egui::Key::Num1),
        VK_2 => Some(egui::Key::Num2),
        VK_3 => Some(egui::Key::Num3),
        VK_4 => Some(egui::Key::Num4),
        VK_5 => Some(egui::Key::Num5),
        VK_6 => Some(egui::Key::Num6),
        VK_7 => Some(egui::Key::Num7),
        VK_8 => Some(egui::Key::Num8),
        VK_9 => Some(egui::Key::Num9),
        VK_A => Some(egui::Key::A),
        VK_B => Some(egui::Key::B),
        VK_C => Some(egui::Key::C),
        VK_D => Some(egui::Key::D),
        VK_E => Some(egui::Key::E),
        VK_F => Some(egui::Key::F),
        VK_G => Some(egui::Key::G),
        VK_H => Some(egui::Key::H),
        VK_I => Some(egui::Key::I),
        VK_J => Some(egui::Key::J),
        VK_K => Some(egui::Key::K),
        VK_L => Some(egui::Key::L),
        VK_M => Some(egui::Key::M),
        VK_N => Some(egui::Key::N),
        VK_O => Some(egui::Key::O),
        VK_P => Some(egui::Key::P),
        VK_Q => Some(egui::Key::Q),
        VK_R => Some(egui::Key::R),
        VK_S => Some(egui::Key::S),
        VK_T => Some(egui::Key::T),
        VK_U => Some(egui::Key::U),
        VK_V => Some(egui::Key::V),
        VK_W => Some(egui::Key::W),
        VK_X => Some(egui::Key::X),
        VK_Y => Some(egui::Key::Y),
        VK_Z => Some(egui::Key::Z),
        VK_F1 => Some(egui::Key::F1),
        VK_F2 => Some(egui::Key::F2),
        VK_F3 => Some(egui::Key::F3),
        VK_F4 => Some(egui::Key::F4),
        VK_F5 => Some(egui::Key::F5),
        VK_F6 => Some(egui::Key::F6),
        VK_F7 => Some(egui::Key::F7),
        VK_F8 => Some(egui::Key::F8),
        VK_F9 => Some(egui::Key::F9),
        VK_F10 => Some(egui::Key::F10),
        VK_F11 => Some(egui::Key::F11),
        VK_F12 => Some(egui::Key::F12),
        VK_F13 => Some(egui::Key::F13),
        VK_F14 => Some(egui::Key::F14),
        VK_F15 => Some(egui::Key::F15),
        VK_F16 => Some(egui::Key::F16),
        VK_F17 => Some(egui::Key::F17),
        VK_F18 => Some(egui::Key::F18),
        VK_F19 => Some(egui::Key::F19),
        VK_F20 => Some(egui::Key::F20),
        VK_F21 => Some(egui::Key::F21),
        VK_F22 => Some(egui::Key::F22),
        VK_F23 => Some(egui::Key::F23),
        VK_F24 => Some(egui::Key::F24),
        _ => None,
    }
}
