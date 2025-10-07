use std::{ffi::c_void, sync::OnceLock};

use windows::{
    Win32::{
        Foundation::HINSTANCE,
        System::{
            LibraryLoader::{DisableThreadLibraryCalls, FreeLibraryAndExitThread},
            SystemServices::DLL_PROCESS_ATTACH,
        },
        UI::WindowsAndMessaging::{MB_OK, MessageBoxA},
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
    unsafe {
        MessageBoxA(None, s!("Hello, world!"), s!("Time to unload!"), MB_OK);
        if let Some(module) = MODULE.get() {
            FreeLibraryAndExitThread(module.0, 0);
        }
    }
}
