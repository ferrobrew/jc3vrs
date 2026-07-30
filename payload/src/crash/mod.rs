//! First-chance crash instrumentation.
//!
//! A vectored exception handler logs the faulting address, a register dump, a module-resolved
//! backtrace of the faulting thread, and a backtrace of every other thread the moment a fatal
//! exception is *raised* -- before any handler unwinds. This covers the case where the game catches a
//! fault itself and turns it into a clean exit: Wine prints no backtrace and the window just
//! vanishes, but the record still lands in the session's crash log (`jc3vrs-crash.log`) -- a
//! dedicated file, separate from `jc3vrs.log` because the handler writes it through a raw file handle
//! with no allocation and no dependence on the tracing subscriber (see below), so a record survives a
//! fault that has already broken the normal logging path; each record head carries a UTC timestamp
//! matching the tracing log's, for cross-correlation. A panic hook does the same for Rust panics,
//! which don't raise an SEH exception and so are invisible to the VEH handler. Each address is
//! resolved to its containing module + offset (`module+0xoff`) via `VirtualQuery`, which works under
//! Wine where `std::backtrace` usually can't symbolize.
//!
//! **The whole logging path is allocation-free, lock-free, and uses no `core::fmt` or `std::io`.**
//! Everything is formatted manually into a fixed [`line::Line`] stack buffer and written with a
//! direct `WriteFile` syscall to a raw `HANDLE` opened once at [`install`]. This is critical: when the
//! original fault has already corrupted memory or is raised in an unusual context, `format!`/`write!`
//! (which marshal arguments through `core::fmt::Arguments`) and `std::fs::File::write_all` (which
//! threads through std's I/O abstraction and thread-locals) fault again -- the handler then immolates
//! itself and masks the very crash it was meant to report (observed in practice). A bare `WriteFile`
//! to a stored handle touches only a single stack array and the OS, so it survives.
//!
//! A single [`IN_HANDLER`] re-entrancy guard covers both the VEH handler and the panic hook, so a
//! fault raised while logging is dropped instead of being logged as a fresh masking record.
//!
//! Repeated identical exceptions (same code + faulting address) are deduplicated: the first
//! occurrence gets a full log, subsequent ones are counted and summarised as a single line. This
//! prevents the log from being flooded with hundreds of identical entries when the game's exception
//! handler retries the faulting instruction. Storms that alternate between several addresses (and
//! so slip past the dedup) are rate-limited instead: past a per-second budget of full dumps, each
//! record shrinks to its head and fault lines.
//!
//! **A record here does not mean the process died.** The VEH observes exceptions first-chance,
//! before any handler runs; the game — and internally-guarded probes like `lstrlen` — routinely
//! catch and survive access violations (observed: Scaleform string probing). The record that
//! killed the process is simply the last one written.

mod breadcrumbs;
mod line;
mod memory;
mod report;
mod threads;

use std::{
    any::Any,
    os::windows::ffi::OsStrExt,
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};

use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        Storage::FileSystem::{
            CreateFileW, FILE_APPEND_DATA, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ,
            FILE_SHARE_WRITE, OPEN_ALWAYS,
        },
        System::{
            Diagnostics::Debug::{
                AddVectoredExceptionHandler, EXCEPTION_POINTERS, RemoveVectoredExceptionHandler,
            },
            SystemInformation::GetTickCount64,
        },
    },
    core::PCWSTR,
};

pub use crate::crash::breadcrumbs::{Phase, mark};
use crate::crash::{
    line::{CRASH_LOG, Line, stamp},
    memory::{readable, resolve_probe_host_bases},
    report::{log_backtrace, record_exception},
};

pub fn install() {
    // Open the crash log with a raw handle. It is a *separate* file from `jc3vrs.log` because the
    // vectored handler writes it directly (raw `WriteFile`, no allocation, no tracing subscriber), so
    // a record survives a fault that has already broken the normal logging path. It lives in the
    // session directory (see [`crate::session`]), opened append-only; each record is timestamped to
    // line up with the (UTC-stamped) tracing log of the run that produced it.
    if let Some(dir_result) = crate::session::dir() {
        match dir_result {
            Ok(dir) => {
                let path = dir.join("jc3vrs-crash.log");
                let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
                wide.push(0);
                // SAFETY: `wide` is a null-terminated UTF-16 path; all other arguments are plain flags.
                let handle = unsafe {
                    CreateFileW(
                        PCWSTR(wide.as_ptr()),
                        FILE_APPEND_DATA.0,
                        FILE_SHARE_READ | FILE_SHARE_WRITE,
                        None,
                        OPEN_ALWAYS,
                        FILE_ATTRIBUTE_NORMAL,
                        None,
                    )
                };
                if let Ok(handle) = handle
                    && !handle.is_invalid()
                {
                    CRASH_LOG.store(handle.0 as isize, Ordering::Relaxed);
                    let mut line = Line::new();
                    line.str("=== session start ");
                    stamp(&mut line);
                    line.str("===").flush();
                }
            }
            Err(e) => {
                // The crash log itself cannot be opened; this runs at startup so just print to stderr.
                eprintln!("crash: could not create session directory: {e}");
            }
        }
    }

    resolve_probe_host_bases();

    let veh = unsafe { AddVectoredExceptionHandler(1, Some(handler)) };
    VEH_HANDLE.store(veh as usize, Ordering::Relaxed);
    // Rust panics unwind/abort instead of raising an SEH exception, so the VEH handler above never
    // sees them. Log the message + a backtrace ourselves before the process dies. The same
    // re-entrancy guard the VEH handler uses covers this hook. Each piece is flushed as its own
    // line, most-reliable first, so a fault while extracting a later piece cannot lose the earlier
    // ones -- the sentinel and location touch nothing heap-derived.
    //
    // The previously installed hook (lib.rs installs the tracing hook before this runs) is
    // chained afterwards, so panics still reach jc3vrs.log: set_hook replaces rather than stacks,
    // which silently disabled the tracing hook until this chaining. The allocation-free crash-log
    // lines land first, so a fault in the richer tracing path cannot mask them.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if IN_HANDLER.swap(true, Ordering::SeqCst) {
            return;
        }
        let mut head = Line::new();
        stamp(&mut head);
        head.str("rust panic").flush();
        let mut line = Line::new();
        line.str("  at ");
        match info.location() {
            Some(loc) => {
                line.str(loc.file()).str(":").dec(loc.line() as u64);
            }
            None => {
                line.str("<unknown location>");
            }
        }
        line.flush();
        if let Some(msg) = panic_message_bytes(info.payload()) {
            Line::new().str("  message: ").bytes(msg).flush();
        }
        unsafe { log_backtrace() };
        previous_hook(info);
        IN_HANDLER.store(false, Ordering::SeqCst);
    }));
    tracing::info!("Crash handler installed");
}

/// Remove the VEH registration and close the crash log on eject. The panic hook needs no
/// counterpart: its registry lives in this DLL's statically linked `std` and unloads with the
/// image, whereas the VEH registration lives in ntdll's process-wide list and would dangle (see
/// [`VEH_HANDLE`]).
pub fn uninstall() {
    let veh = VEH_HANDLE.swap(0, Ordering::Relaxed);
    if veh != 0 {
        // SAFETY: `veh` is the registration handle returned by `AddVectoredExceptionHandler` in
        // `install`, removed at most once via the swap above.
        unsafe { RemoveVectoredExceptionHandler(veh as *mut std::ffi::c_void) };
    }
    let mut line = Line::new();
    line.str("=== session end (uninjected) ");
    stamp(&mut line);
    line.str("===").flush();
    let raw = CRASH_LOG.swap(0, Ordering::Relaxed);
    if raw != 0 {
        // SAFETY: `raw` is the append-mode handle opened in `install`, closed at most once via the
        // swap above; the handler treats a zero handle as "not opened" and skips writing.
        unsafe {
            let _ = CloseHandle(HANDLE(raw as *mut std::ffi::c_void));
        }
    }
    tracing::info!("Crash handler uninstalled");
}

const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

/// The registration handle from `AddVectoredExceptionHandler`, so [`uninstall`] can remove it on
/// eject. Leaving the registration behind is fatal: ntdll's handler list would keep a pointer into
/// the unmapped DLL, and the game's next routine first-chance exception (its SEH probes fire
/// constantly) would dispatch straight into freed memory — observed as a consistent crash within a
/// minute of uninjection.
static VEH_HANDLE: AtomicUsize = AtomicUsize::new(0);

/// Reentrancy guard: set while the VEH handler or panic hook is running. If the logging code itself
/// triggers an exception (or a panic fires mid-handler), the recursive entry sees this flag and
/// returns immediately, preventing an infinite loop of self-inflicted exceptions that would mask the
/// original fault. Cleared after the handler finishes, so genuinely different exceptions on other
/// threads are still logged.
static IN_HANDLER: AtomicBool = AtomicBool::new(false);

/// When the VEH guard was last taken (`GetTickCount64` milliseconds), for jam recovery: a handler
/// entry that faulted mid-record never clears [`IN_HANDLER`], so a later entry finding the guard
/// held longer than [`HANDLER_JAM_MS`] steals it rather than dropping every subsequent record.
static HANDLER_TAKEN_AT_MS: AtomicU64 = AtomicU64::new(0);

/// How long the VEH guard may be held before it is considered jammed. A healthy record (full
/// thread dump included) completes in well under a second.
const HANDLER_JAM_MS: u64 = 2000;

unsafe extern "system" fn handler(info: *mut EXCEPTION_POINTERS) -> i32 {
    if IN_HANDLER.swap(true, Ordering::SeqCst) {
        // Held by an earlier entry. If it has been held implausibly long, that entry faulted
        // mid-record and never cleared the guard (an AV inside the handler cannot unwind back
        // through it) — without recovery, every later exception in the process would be silently
        // dropped, including the one that finally kills it. Steal the guard and record anyway.
        let taken_ms = HANDLER_TAKEN_AT_MS.load(Ordering::Relaxed);
        if unsafe { GetTickCount64() }.saturating_sub(taken_ms) < HANDLER_JAM_MS {
            return EXCEPTION_CONTINUE_SEARCH;
        }
        Line::new()
            .str("  (recovered a jammed handler guard: a previous record faulted mid-write)")
            .flush();
    }
    HANDLER_TAKEN_AT_MS.store(unsafe { GetTickCount64() }, Ordering::Relaxed);
    // A panic anywhere in the logging tree would otherwise unwind into this `extern "system"`
    // boundary and hit the compiler's abort pad — a ud2 that raises ILLEGAL_INSTRUCTION *from
    // inside the handler*, which Wine's dispatch then retries in a loop until the process dies
    // (observed in-game: 200+ identical C000001D records at our own module in 11 ms). Catch the
    // panic instead, and log its message through the same allocation-free writer so the culprit
    // names itself.
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        unsafe { record_exception(info) };
    }));
    if let Err(payload) = panicked {
        let mut line = Line::new();
        line.str("  (the crash handler itself panicked while logging; record incomplete: ");
        match panic_message_bytes(payload.as_ref()) {
            Some(msg) => {
                line.bytes(msg);
            }
            None => {
                line.str("<no message>");
            }
        }
        line.str(")").flush();
    }
    IN_HANDLER.store(false, Ordering::SeqCst);
    EXCEPTION_CONTINUE_SEARCH
}

/// Extract the panic message payload as bytes without `core::fmt`, probing every heap pointer
/// before it is dereferenced. The payload `Box` and (for `String`s) the backing buffer live on the
/// heap, and a panic raised *because of* heap corruption can hand the hook a payload whose
/// pointers are garbage -- dereferencing them would fault the handler and mask the panic (std
/// itself has been observed dying this way *before* the hook, formatting the message into a
/// corrupted heap; when the allocation survives but the bytes are bad, this probing is what keeps
/// the hook alive). The type checks are safe without probing: `is`/`downcast_ref` only read the
/// vtable, which lives in this module's read-only image. Returns raw bytes rather than `&str`
/// because a corrupted buffer need not hold valid UTF-8, and [`line::Line`] only copies bytes anyway.
fn panic_message_bytes(payload: &dyn Any) -> Option<&[u8]> {
    let data = payload as *const dyn Any as *const u8 as usize;
    let (ptr, len) = if payload.is::<&str>() {
        if !unsafe { readable(data, std::mem::size_of::<&str>()) } {
            return None;
        }
        let s: &str = payload.downcast_ref::<&str>()?;
        (s.as_ptr(), s.len())
    } else if payload.is::<String>() {
        if !unsafe { readable(data, std::mem::size_of::<String>()) } {
            return None;
        }
        // `as_ptr`/`len` only read the `String` header, which the probe above covered; the buffer
        // itself is probed below.
        let s: &String = payload.downcast_ref::<String>()?;
        (s.as_ptr(), s.len())
    } else {
        return None;
    };
    if len == 0 {
        return Some(&[]);
    }
    // Clamp to the line capacity before probing: `readable` requires the span to fit one region,
    // and `Line` truncates past its buffer anyway.
    let len = len.min(512);
    if !unsafe { readable(ptr as usize, len) } {
        return None;
    }
    // SAFETY: probed readable just above; the lifetime is tied to `payload`, which outlives the
    // hook body that consumes the slice.
    Some(unsafe { std::slice::from_raw_parts(ptr, len) })
}
