use std::{
    ffi::c_void,
    sync::{Mutex, OnceLock},
};

use detours_macro::detour;
use re_utilities::{
    ThreadSuspender,
    hook_library::{HookLibraries, HookLibrary},
};
use windows::{
    Win32::{
        Foundation::HINSTANCE,
        System::{
            LibraryLoader::{DisableThreadLibraryCalls, FreeLibraryAndExitThread},
            SystemServices::DLL_PROCESS_ATTACH,
        },
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, VK_F5},
            WindowsAndMessaging::{MB_OK, MessageBoxA},
        },
    },
    core::s,
};

struct ThisModule(HINSTANCE);
unsafe impl Send for ThisModule {}
unsafe impl Sync for ThisModule {}

static MODULE: OnceLock<ThisModule> = OnceLock::new();

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
    install();
}

fn shutdown() {
    static SHUTDOWN: OnceLock<bool> = OnceLock::new();
    if SHUTDOWN.get().is_some() {
        return;
    }
    SHUTDOWN.set(true).unwrap();

    uninstall();

    unsafe {
        MessageBoxA(None, s!("Hello, world!"), s!("Time to unload!"), MB_OK);
        std::thread::sleep(std::time::Duration::from_secs(1));
        if let Some(module) = MODULE.get() {
            FreeLibraryAndExitThread(module.0, 0);
        }
    }
}

static HOOK_LIBRARY: OnceLock<MainHookLibraries> = OnceLock::new();
struct MainHookLibraries {
    _patcher: Mutex<re_utilities::Patcher>,
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
            println!("Failed to enable hook libraries: {}", e);
            return;
        }
    };
    let _ = HOOK_LIBRARY.set(MainHookLibraries {
        _patcher: Mutex::new(patcher),
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
            .set_enabled(&mut hook_libraries._patcher.lock().unwrap(), false)
    });
}

#[detour(address = 0x143_C7B_6A0)]
fn game_update_hook(game: *const c_void) -> bool {
    unsafe {
        if GetAsyncKeyState(VK_F5.0 as _) != 0 {
            std::thread::spawn(shutdown);
        }
    }
    GAME_UPDATE_HOOK.get().unwrap().call(game)
}
