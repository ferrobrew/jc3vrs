use std::{ffi::OsString, os::windows::ffi::OsStringExt as _, path::PathBuf, sync::OnceLock};

use windows::Win32::{
    Foundation::{HMODULE, MAX_PATH},
    System::LibraryLoader::{
        FreeLibraryAndExitThread, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
        GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleFileNameW, GetModuleHandleExW,
    },
};

struct ThisModule(HMODULE);
unsafe impl Send for ThisModule {}
unsafe impl Sync for ThisModule {}

static MODULE: OnceLock<ThisModule> = OnceLock::new();

pub fn set(module: HMODULE) {
    MODULE.set(ThisModule(module)).ok();
}

pub fn exit() {
    if PINNED.load(std::sync::atomic::Ordering::Acquire) {
        // Pinned because teardown could not prove the image was clear; unmapping now is the wedge
        // that pinning exists to avoid. The thread simply returns instead.
        return;
    }
    if let Some(module) = MODULE.get() {
        unsafe {
            FreeLibraryAndExitThread(module.0, 0);
        }
    }
}

/// Pin the payload image so it is never unmapped, and neuter [`exit`].
///
/// The escape hatch for a teardown that cannot prove the image is clear -- a worker that would not
/// stop, most usefully. A thread still executing in an unmapped image has no return address: it
/// parks forever holding whatever locks it took, and the process wedges with no runnable threads and
/// nothing written to the log. Leaking this DLL for the rest of the game's run is a far smaller cost,
/// and the player ends it by quitting.
///
/// Reinjection after this still works -- the injector loads a fresh copy under a new name.
pub fn pin() {
    let Some(module) = MODULE.get() else {
        return;
    };
    PINNED.store(true, std::sync::atomic::Ordering::Release);
    let mut pinned = HMODULE::default();
    // SAFETY: `module.0` is this DLL's own handle, recorded at `DllMain`. `FROM_ADDRESS` is not used;
    // the handle form takes the module name, so pass our own path-backed handle as the name pointer
    // is not required for the PIN flag with a live handle.
    let _ = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_PIN | GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
            windows::core::PCWSTR(module.0.0 as *const u16),
            &mut pinned,
        )
    };
    tracing::error!("the payload image is pinned and will not unload for this run of the game");
}

/// Whether [`pin`] was called; [`exit`] becomes a no-op so nothing frees a pinned image.
static PINNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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
