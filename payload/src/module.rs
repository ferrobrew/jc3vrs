use std::{ffi::OsString, os::windows::ffi::OsStringExt as _, path::PathBuf, sync::OnceLock};

use windows::Win32::{
    Foundation::{HMODULE, MAX_PATH},
    System::LibraryLoader::{FreeLibraryAndExitThread, GetModuleFileNameW},
};

struct ThisModule(HMODULE);
unsafe impl Send for ThisModule {}
unsafe impl Sync for ThisModule {}

static MODULE: OnceLock<ThisModule> = OnceLock::new();

pub fn set(module: HMODULE) {
    MODULE.set(ThisModule(module)).ok();
}

pub fn exit() {
    if let Some(module) = MODULE.get() {
        unsafe {
            FreeLibraryAndExitThread(module.0, 0);
        }
    }
}

pub fn get_path() -> Option<PathBuf> {
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
