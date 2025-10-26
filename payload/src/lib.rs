use std::{
    ffi::{OsString, c_void},
    os::windows::ffi::OsStringExt as _,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

use detours_macro::detour;
use re_utilities::{
    ThreadSuspender,
    hook_library::{HookLibraries, HookLibrary},
};
use tracing_subscriber::{Layer as _, layer::SubscriberExt as _, util::SubscriberInitExt as _};
use windows::Win32::{
    Foundation::{HINSTANCE, MAX_PATH},
    System::{
        Console::{
            AllocConsole, ENABLE_PROCESSED_OUTPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING, FreeConsole,
            GetStdHandle, STD_OUTPUT_HANDLE, SetConsoleMode,
        },
        LibraryLoader::{DisableThreadLibraryCalls, FreeLibraryAndExitThread, GetModuleFileNameW},
        SystemServices::DLL_PROCESS_ATTACH,
    },
    UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_F5},
};

struct ThisModule(HINSTANCE);
unsafe impl Send for ThisModule {}
unsafe impl Sync for ThisModule {}

static MODULE: OnceLock<ThisModule> = OnceLock::new();

fn get_module_path() -> Option<PathBuf> {
    unsafe {
        if let Some(module) = MODULE.get() {
            let mut buffer = [0u16; MAX_PATH as usize];
            let result = GetModuleFileNameW(module.0, &mut buffer);
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
pub extern "system" fn DllMain(module: HINSTANCE, reason: u32, _unk: *mut c_void) -> bool {
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
        HookLibraries::new([game_update_hook_library()]).enable(&mut patcher)
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

fn game_update_hook_library() -> HookLibrary {
    HookLibrary::new().with_static_binder(&GAME_UPDATE_HOOK_BINDER)
}

fn uninstall() {
    let hook_libraries = HOOK_LIBRARY.get().unwrap();
    let _ = ThreadSuspender::for_block(|| {
        hook_libraries
            .hook_libraries
            .set_enabled(&mut hook_libraries.patcher.lock().unwrap(), false)
    });
}

#[detour(address = 0x143_C7B_6A0)]
fn game_update_hook(game: *const c_void) -> bool {
    unsafe {
        if GetAsyncKeyState(VK_F5.0 as _) != 0 {
            shutdown();
        }
    }
    GAME_UPDATE_HOOK.get().unwrap().call(game)
}
