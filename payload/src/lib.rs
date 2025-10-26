use std::{
    ffi::{OsString, c_void},
    os::windows::ffi::OsStringExt as _,
    path::PathBuf,
    sync::OnceLock,
};

use anyhow::Context;
use detours_macro::detour;
use jc3gi::{
    graphics_engine::{
        camera::Camera,
        device::Device,
        graphics_engine::{ActiveCursor, GraphicsEngine, get_graphics_params},
    },
    types::math::Matrix4,
};
use parking_lot::Mutex;
use re_utilities::{
    ThreadSuspender,
    hook_library::{HookLibraries, HookLibrary},
};
use tracing_subscriber::{Layer as _, layer::SubscriberExt as _, util::SubscriberInitExt as _};
use windows::Win32::{
    Foundation::{HMODULE, HWND, LPARAM, LRESULT, MAX_PATH, WPARAM},
    System::{
        Console::{
            AllocConsole, ENABLE_PROCESSED_OUTPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING, FreeConsole,
            GetStdHandle, STD_OUTPUT_HANDLE, SetConsoleMode,
        },
        LibraryLoader::{DisableThreadLibraryCalls, FreeLibraryAndExitThread, GetModuleFileNameW},
        SystemServices::DLL_PROCESS_ATTACH,
    },
    UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VIRTUAL_KEY, VK_F5, VK_F6, VK_F7},
};

struct ThisModule(HMODULE);
unsafe impl Send for ThisModule {}
unsafe impl Sync for ThisModule {}

static MODULE: OnceLock<ThisModule> = OnceLock::new();

fn get_module_path() -> Option<PathBuf> {
    unsafe {
        if let Some(module) = MODULE.get() {
            let mut buffer = [0u16; MAX_PATH as usize];
            let result = GetModuleFileNameW(Some(module.0), &mut buffer);
            if result > 0 {
                let path_os_string = OsString::from_wide(&buffer[..result as usize]);
                return Some(PathBuf::from(path_os_string));
            }
        }
    }
    None
}

fn setup_tracing() {
    let module_path = get_module_path();
    let log_file_path = module_path
        .as_ref()
        .and_then(|path| path.parent())
        .map(|parent| parent.join("jc3vrs.log"));

    let log_file = log_file_path.and_then(|path| std::fs::File::create(&path).ok());

    let env_filter = tracing_subscriber::EnvFilter::from_default_env()
        .add_directive(tracing_subscriber::filter::LevelFilter::INFO.into());

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stdout)
                .with_filter(env_filter.clone()),
        )
        .with(log_file.map(|file| {
            tracing_subscriber::fmt::layer()
                .with_writer(file)
                .with_ansi(false)
                .with_filter(env_filter)
        }))
        .init();
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "system" fn DllMain(module: HMODULE, reason: u32, _unk: *mut c_void) -> bool {
    if reason == DLL_PROCESS_ATTACH {
        unsafe {
            DisableThreadLibraryCalls(module).ok();
            MODULE.set(ThisModule(module)).ok();
        };
    }
    true
}

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "system" fn run(_: *mut c_void) {
    unsafe {
        AllocConsole().ok();
        if let Ok(handle) = GetStdHandle(STD_OUTPUT_HANDLE) {
            SetConsoleMode(
                handle,
                ENABLE_VIRTUAL_TERMINAL_PROCESSING | ENABLE_PROCESSED_OUTPUT,
            )
            .ok();
        }
    }

    setup_tracing();
    tracing::info!("JC3VRS startup");

    install();
}

fn shutdown() {
    static SHUTDOWN: OnceLock<bool> = OnceLock::new();
    if SHUTDOWN.get().is_some() {
        return;
    }
    SHUTDOWN.set(true).unwrap();

    tracing::info!("Shutting down");
    std::thread::spawn(|| {
        EguiState::uninit();
        tracing::info!("Uninstalling hooks");
        uninstall();

        unsafe {
            std::thread::sleep(std::time::Duration::from_millis(100));
            tracing::info!("Ejecting");
            FreeConsole().ok();
            if let Some(module) = MODULE.get() {
                FreeLibraryAndExitThread(module.0, 0);
            }
        }
    });
}

static HOOK_LIBRARY: OnceLock<MainHookLibraries> = OnceLock::new();
struct MainHookLibraries {
    patcher: Mutex<re_utilities::Patcher>,
    hook_libraries: HookLibraries,
}
unsafe impl Send for MainHookLibraries {}
unsafe impl Sync for MainHookLibraries {}

fn install() {
    let mut patcher = re_utilities::Patcher::new();
    let hook_libraries = ThreadSuspender::for_block(|| {
        HookLibraries::new([
            game_update_hook_library(),
            camera_hook_library(),
            graphics_hook_library(),
            wndproc_hook_library(),
        ])
        .enable(&mut patcher)
    });
    let hook_libraries = match hook_libraries {
        Ok(hook_libraries) => hook_libraries,
        Err(e) => {
            tracing::error!("Failed to enable hook libraries: {e:?}");
            return;
        }
    };
    let _ = HOOK_LIBRARY.set(MainHookLibraries {
        patcher: Mutex::new(patcher),
        hook_libraries,
    });
}

fn uninstall() {
    let hook_libraries = HOOK_LIBRARY.get().unwrap();
    let _ = ThreadSuspender::for_block(|| {
        hook_libraries
            .hook_libraries
            .set_enabled(&mut hook_libraries.patcher.lock(), false)
    });
}

struct EguiState {
    start_time: std::time::Instant,
    egui_context: egui::Context,
    egui_renderer: egui_directx11::Renderer,
    renderer_output: Option<egui_directx11::RendererOutput>,
    game_input_state: Option<GameInputState>,
    events: Vec<egui::Event>,
}
struct GameInputState {
    input_was_enabled: bool,
    active_cursor: ActiveCursor,
}
static EGUI_STATE: Mutex<Option<EguiState>> = Mutex::new(None);
impl EguiState {
    pub fn new() -> anyhow::Result<Self> {
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
        })
    }

    pub fn init() -> anyhow::Result<()> {
        EGUI_STATE.lock().replace(Self::new()?);
        tracing::info!("Initialized egui");
        Ok(())
    }

    pub fn uninit() {
        EGUI_STATE.lock().take();
        tracing::info!("Uninitialized egui");
    }

    pub fn run(&mut self, callback: impl FnMut(&egui::Context)) {
        let params = unsafe { get_graphics_params() };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_max(
                Default::default(),
                egui::Pos2::new(params.m_Width as f32, params.m_Height as f32),
            )),
            time: Some(self.start_time.elapsed().as_secs_f64()),
            focused: self.game_input_state.is_some(),
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };
        let egui_output = self.egui_context.run(input, callback);
        let (renderer_output, platform_output, _) = egui_directx11::split_output(egui_output);

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
                1.0,
            ) {
                tracing::error!("Failed to render egui: {e:?}");
            }
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
                input_device_manager.enabled = game_input_state.input_was_enabled;
                graphics_engine.m_ActiveCursor = game_input_state.active_cursor;
            } else {
                self.game_input_state = Some(GameInputState {
                    input_was_enabled: input_device_manager.enabled,
                    active_cursor: graphics_engine.m_ActiveCursor,
                });

                input_device_manager.enabled = false;
            }
        }
    }

    pub fn is_input_captured(&self) -> bool {
        self.game_input_state.is_some()
    }

    pub fn wndproc(&mut self, _hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) {
        use windows::Win32::UI::WindowsAndMessaging::*;

        if !self.is_input_captured() {
            return;
        }

        let wparam = wparam.0;
        let lparam = lparam.0;

        match msg {
            WM_MOUSEMOVE => {
                let x = (lparam & 0xFFFF) as i16 as f32;
                let y = ((lparam >> 16) & 0xFFFF) as i16 as f32;
                self.events
                    .push(egui::Event::PointerMoved(egui::Pos2::new(x, y)));
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
                self.events.push(egui::Event::PointerButton {
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
                self.events.push(egui::Event::PointerButton {
                    pos: egui::Pos2::new(x, y),
                    button,
                    pressed: false,
                    modifiers: get_modifiers(),
                });
            }

            WM_MOUSEWHEEL => {
                let delta = (wparam >> 16) as i16 as f32 / WHEEL_DELTA as f32;
                self.events.push(egui::Event::MouseWheel {
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
                        self.events.push(egui::Event::Copy);
                    } else if is_cut_command(modifiers, key) {
                        self.events.push(egui::Event::Cut);
                    } else if is_paste_command(modifiers, key) {
                        push_paste_command_if_available(&mut self.events);
                    } else {
                        self.events.push(egui::Event::Key {
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
                    self.events.push(egui::Event::Key {
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
                    self.events.push(egui::Event::Text(ch.to_string()));
                }
            }

            WM_PASTE => {
                push_paste_command_if_available(&mut self.events);
            }

            _ => {}
        }

        fn get_modifiers() -> egui::Modifiers {
            use windows::Win32::UI::Input::KeyboardAndMouse::{
                GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT,
            };
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
    }
}

static FREEZE_TRANSFORM: Mutex<Option<Matrix4>> = Mutex::new(None);

fn game_update_hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&GAME_UPDATE_BINDER)
}

fn initialize_in_game_thread() -> anyhow::Result<()> {
    static INITIALIZED: OnceLock<bool> = OnceLock::new();
    if INITIALIZED.get().is_some() {
        return Ok(());
    }
    INITIALIZED.set(true).unwrap();

    EguiState::init()?;
    tracing::info!("Initialized in game thread");

    Ok(())
}

#[detour(address = 0x143_C7B_6A0)]
fn game_update(game: *const c_void) -> bool {
    fn is_pressed(key: VIRTUAL_KEY) -> bool {
        static LAST_INPUT: Mutex<Option<std::time::Instant>> = Mutex::new(None);
        if LAST_INPUT
            .lock()
            .is_some_and(|last_input| last_input.elapsed() < std::time::Duration::from_millis(250))
        {
            return false;
        }

        let output = unsafe { GetAsyncKeyState(key.0 as _) != 0 };

        if output {
            *LAST_INPUT.lock() = Some(std::time::Instant::now());
        }

        output
    }

    if let Err(e) = initialize_in_game_thread() {
        tracing::error!("Failed to initialize in game thread, shutting down: {e:?}");
        shutdown();
    }

    if let Some(egui_state) = EGUI_STATE.lock().as_mut() {
        egui_state.run(|ctx| {
            egui::Window::new("Hello world!").show(ctx, |ui| {
                ui.label("Hi from egui!");
                ui.spinner();
            });
        });
    }

    if is_pressed(VK_F5) {
        shutdown();
    } else if is_pressed(VK_F6) {
        if let Some(egui_state) = EGUI_STATE.lock().as_mut() {
            egui_state.toggle_game_input_capture();
        }
    } else if is_pressed(VK_F7) {
        unsafe {
            let is_frozen = FREEZE_TRANSFORM.lock().is_some();
            if is_frozen {
                tracing::info!("Unfreezing position");
                FREEZE_TRANSFORM.lock().take();
            } else if let Some(cm) = jc3gi::graphics_engine::camera_manager::CameraManager::get()
                && let Some(camera) = cm.m_ActiveCamera.as_mut()
            {
                tracing::info!(position=?camera.m_TransformF.data[12..15], "Freezing position");
                FREEZE_TRANSFORM.lock().replace(camera.m_TransformF);
            }
        }
    }

    GAME_UPDATE.get().unwrap().call(game)
}

fn camera_hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&CAMERA_UPDATE_RENDER_BINDER)
}

#[detour(address = 0x143_2EB_C70)]
fn camera_update_render(camera: *mut Camera, dt: f32, dtf: f32) {
    unsafe {
        if let Some(cm) = jc3gi::graphics_engine::camera_manager::CameraManager::get()
            && camera == cm.m_ActiveCamera
            && let Some(freeze_transform) = *FREEZE_TRANSFORM.lock()
            && let Some(camera) = camera.as_mut()
        {
            camera.m_TransformT0 = freeze_transform;
            camera.m_TransformT1 = freeze_transform;
        }
    }
    CAMERA_UPDATE_RENDER.get().unwrap().call(camera, dt, dtf);
}

fn graphics_hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&GRAPHICS_FLIP_BINDER)
}

#[detour(address = 0x145_34B_870)]
fn graphics_flip(device: *mut Device) -> u64 {
    if let Some(egui_state) = EGUI_STATE.lock().as_mut() {
        egui_state.render();
    }

    GRAPHICS_FLIP.get().unwrap().call(device)
}

fn wndproc_hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&WNDPROC_BINDER)
}

#[detour(address = 0x143_213_830)]
fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if let Some(egui_state) = EGUI_STATE.lock().as_mut() {
        egui_state.wndproc(hwnd, msg, wparam, lparam);
    }

    WNDPROC.get().unwrap().call(hwnd, msg, wparam, lparam)
}
