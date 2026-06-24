//! First-chance crash instrumentation.
//!
//! A vectored exception handler logs the faulting address + a module-resolved backtrace the moment a
//! fatal exception is *raised* -- before any handler unwinds. This covers the case where the game
//! catches a fault itself and turns it into a clean exit: Wine prints no backtrace and the window
//! just vanishes, but the record still lands in `jc3vrs.log`. A panic hook does the same for Rust
//! panics, which don't raise an SEH exception and so are invisible to the VEH handler. Each address
//! is resolved to its containing module + offset (`module+0xoff`), which works under Wine where
//! `std::backtrace` usually can't symbolize.
//!
//! All output goes through [`log_raw`], which writes directly to the log file via `writeln!`,
//! bypassing the `tracing` subscriber entirely. This is critical: `tracing::error!` acquires the
//! subscriber's internal mutex and touches thread-local state, both of which can be poisoned or
//! unavailable inside a VEH handler or panic hook. Calling `tracing` from a crash handler causes a
//! reentrant panic that masks the original error -- exactly the failure that motivated this rewrite.

use std::io::Write;

use windows::{
    Win32::{
        Foundation::HMODULE,
        System::{
            Diagnostics::Debug::{
                AddVectoredExceptionHandler, EXCEPTION_POINTERS, EXCEPTION_RECORD,
                RtlCaptureStackBackTrace,
            },
            LibraryLoader::{
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleFileNameW,
                GetModuleHandleExW,
            },
        },
    },
    core::PCWSTR,
};

const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

/// Fatal codes worth recording -- skip C++ exceptions (0xE06D7363), debug/breakpoint events and
/// benign first-chance ones (stack guard-page growth is 0x80000001, not in this list).
const FATAL_CODES: &[u32] = &[
    0xC0000005, // ACCESS_VIOLATION
    0xC000001D, // ILLEGAL_INSTRUCTION
    0xC0000094, // INTEGER_DIVIDE_BY_ZERO
    0xC0000096, // PRIVILEGED_INSTRUCTION
    0xC00000FD, // STACK_OVERFLOW
];

/// The log file for crash-handler writes. Opened at [`install`] time as a raw `File` so it works
/// even when the `tracing` subscriber is poisoned or unavailable. Uses `try_lock` so a reentrant
/// call (e.g. a panic inside the crash handler itself) skips the write instead of deadlocking.
static CRASH_LOG: parking_lot::Mutex<Option<std::fs::File>> = parking_lot::Mutex::new(None);

pub fn install() {
    // Open the log file for direct writes, sharing the same `jc3vrs.log` the tracing subscriber
    // writes to. Append mode so crash records are appended to the existing log.
    if let Some(path) = crate::module::get_path()
        .as_ref()
        .and_then(|path| path.parent())
        .map(|parent| parent.join("jc3vrs.log"))
        .and_then(|path| {
            std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(&path)
                .ok()
        })
    {
        *CRASH_LOG.lock() = Some(path);
    }

    unsafe { AddVectoredExceptionHandler(1, Some(handler)) };
    // Rust panics unwind/abort instead of raising an SEH exception, so the VEH handler above never
    // sees them. Log the message + a module-resolved backtrace ourselves before the process dies.
    std::panic::set_hook(Box::new(|info| {
        log_raw(&format!("rust panic: {info}"));
        unsafe { log_backtrace() };
    }));
    tracing::info!("Crash handler installed");
}

/// Write a line directly to the crash log file, bypassing `tracing`. Safe to call from VEH handlers
/// and panic hooks -- uses `try_lock` so a reentrant call skips the write instead of deadlocking,
/// and never touches `tracing`'s subscriber mutex or thread-local state.
fn log_raw(line: &str) {
    if let Some(mut guard) = CRASH_LOG.try_lock()
        && let Some(file) = guard.as_mut()
    {
        let _ = writeln!(file, "{line}");
    }
}

/// Reentrancy guard: set when the VEH handler is running. If the handler's own logging code
/// triggers an exception (e.g. `GetModuleHandleExW` faults under Wine), the recursive call sees
/// this flag and returns immediately, preventing an infinite loop of self-inflicted exceptions.
/// The flag is never cleared once set by a recursive fault -- this is intentional: if logging
/// itself crashes, we've already logged what we can, and further attempts would only repeat the
/// same crash.
static IN_HANDLER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

unsafe extern "system" fn handler(info: *mut EXCEPTION_POINTERS) -> i32 {
    if IN_HANDLER.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    unsafe {
        if let Some(info) = info.as_ref()
            && let Some(rec) = info.ExceptionRecord.as_ref()
            && FATAL_CODES.contains(&(rec.ExceptionCode.0 as u32))
        {
            log_record(rec);
        }
    }
    IN_HANDLER.store(false, std::sync::atomic::Ordering::SeqCst);
    EXCEPTION_CONTINUE_SEARCH
}

/// Resolve `addr` to (module basename, offset within that module), or `None` if it isn't inside a
/// loaded module.
unsafe fn resolve(addr: usize) -> Option<(String, usize)> {
    unsafe {
        let mut hmod = HMODULE::default();
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(addr as *const u16),
            &mut hmod,
        )
        .ok()?;
        let mut buf = [0u16; 260];
        let len = GetModuleFileNameW(Some(hmod), &mut buf) as usize;
        let path = String::from_utf16_lossy(&buf[..len]);
        let name = path.rsplit(['\\', '/']).next().unwrap_or("?").to_string();
        Some((name, addr.wrapping_sub(hmod.0 as usize)))
    }
}

/// Log one frame as `module+offset` where it resolves, otherwise the raw address.
unsafe fn log_frame(at: &str, addr: usize) {
    unsafe {
        match resolve(addr) {
            Some((module, offset)) => {
                log_raw(&format!(
                    "  {at}: module={module} offset={offset:#X} addr={addr:#018X}"
                ));
            }
            None => log_raw(&format!("  {at}: addr={addr:#018X}")),
        }
    }
}

/// Capture the current call stack and log each frame, resolved to module+offset.
unsafe fn log_backtrace() {
    unsafe {
        let mut raw = [std::ptr::null_mut::<std::ffi::c_void>(); 48];
        let n = RtlCaptureStackBackTrace(0, &mut raw, None) as usize;
        for (i, f) in raw[..n.min(raw.len())].iter().enumerate() {
            log_frame(&format!("bt[{i:02}]"), *f as usize);
        }
    }
}

unsafe fn log_record(rec: &EXCEPTION_RECORD) {
    unsafe {
        let code = rec.ExceptionCode.0 as u32;
        let (access_kind, access_addr) = if rec.NumberParameters >= 2 {
            let kind = match rec.ExceptionInformation[0] {
                0 => "read",
                1 => "write",
                8 => "exec",
                _ => "?",
            };
            (kind, rec.ExceptionInformation[1])
        } else {
            ("n/a", 0)
        };

        log_raw(&format!(
            "fatal exception: code={code:#010X} access={access_kind} access_addr={access_addr:#018X}"
        ));

        // The faulting instruction, then the captured stack.
        log_frame("fault", rec.ExceptionAddress as usize);
        log_backtrace();
    }
}
