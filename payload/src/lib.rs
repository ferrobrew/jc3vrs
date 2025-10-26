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
        graphics_engine::{GraphicsEngine, get_graphics_params},
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
    Foundation::{HMODULE, MAX_PATH},
    System::{
        Console::{
            AllocConsole, ENABLE_PROCESSED_OUTPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING, FreeConsole,
            GetStdHandle, STD_OUTPUT_HANDLE, SetConsoleMode,
        },
        LibraryLoader::{DisableThreadLibraryCalls, FreeLibraryAndExitThread, GetModuleFileNameW},
        SystemServices::DLL_PROCESS_ATTACH,
    },
    UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VIRTUAL_KEY, VK_F5, VK_F7},
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

    pub fn run(callback: impl FnMut(&egui::Context)) {
        let mut state = EGUI_STATE.lock();
        let Some(state) = state.as_mut() else {
            return;
        };
        let params = unsafe { get_graphics_params() };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_max(
                Default::default(),
                egui::Pos2::new(params.m_Width as f32, params.m_Height as f32),
            )),
            time: Some(state.start_time.elapsed().as_secs_f64()),
            ..Default::default()
        };
        let egui_output = state.egui_context.run(input, callback);
        let (renderer_output, _platform_output, _) = egui_directx11::split_output(egui_output);
        state.renderer_output = Some(renderer_output);
    }

    pub fn render() {
        let mut state = EGUI_STATE.lock();
        let Some(state) = state.as_mut() else {
            return;
        };
        let Some(renderer_output) = state.renderer_output.take() else {
            return;
        };
        unsafe {
            let Some(graphics_engine) = GraphicsEngine::get() else {
                return;
            };
            let Some(device) = graphics_engine.m_Device.as_mut() else {
                return;
            };
            let Some(context) = device.m_Context.as_mut() else {
                return;
            };
            let Some(backbuffer) = device.m_BackBuffer.as_mut() else {
                return;
            };
            if let Err(e) = state.egui_renderer.render(
                &context.m_Context,
                &backbuffer.m_RTV,
                &state.egui_context,
                renderer_output,
                1.0,
            ) {
                tracing::error!("Failed to render egui: {e:?}");
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

    EguiState::run(|ctx| {
        egui::Window::new("Hello world!").show(ctx, |ui| {
            ui.spinner();
        });
    });

    if is_pressed(VK_F5) {
        shutdown();
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
    EguiState::render();

    GRAPHICS_FLIP.get().unwrap().call(device)
}
