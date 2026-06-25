//! First-chance crash instrumentation.
//!
//! A vectored exception handler logs the faulting address, a register dump, and a
//! module-resolved backtrace the moment a fatal exception is *raised* -- before any handler
//! unwinds. This covers the case where the game catches a fault itself and turns it into a clean
//! exit: Wine prints no backtrace and the window just vanishes, but the record still lands in
//! `jc3vrs.log`. A panic hook does the same for Rust panics, which don't raise an SEH exception and
//! so are invisible to the VEH handler. Each address is resolved to its containing module +
//! offset (`module+0xoff`) via `VirtualQuery`, which works under Wine where `std::backtrace`
//! usually can't symbolize.
//!
//! **The whole logging path is allocation-free and uses no `core::fmt` machinery.** Everything is
//! formatted manually into a fixed [`Line`] stack buffer and written to the file with a single
//! `write_all`. This is critical: when the original fault has already smashed or exhausted the
//! stack, `format!`/`write!` fault again while marshalling their arguments through
//! `core::fmt::Arguments` -- the handler then immolates itself and masks the very crash it was
//! meant to report (observed in practice as a recursive access violation inside `UpperHex::fmt`).
//! Manual formatting touches only a single stack array and the file handle, so it survives a
//! corrupt stack and a poisoned allocator. The writes also bypass the `tracing` subscriber, whose
//! internal mutex and thread-local state can be unavailable inside a VEH handler or panic hook.
//!
//! A single [`IN_HANDLER`] re-entrancy guard covers both the VEH handler and the panic hook, so a
//! fault raised while logging is dropped instead of being logged as a fresh masking record.
//!
//! Repeated identical exceptions (same code + faulting address) are deduplicated: the first
//! occurrence gets a full log, subsequent ones are counted and summarised as a single line. This
//! prevents the log from being flooded with hundreds of identical entries when the game's
//! exception handler retries the faulting instruction.

use std::{
    io::Write,
    sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};

use windows::Win32::{
    Foundation::HMODULE,
    System::{
        Diagnostics::Debug::{
            AddVectoredExceptionHandler, CONTEXT, EXCEPTION_POINTERS, EXCEPTION_RECORD,
            RtlCaptureStackBackTrace,
        },
        LibraryLoader::GetModuleFileNameW,
        Memory::{MEMORY_BASIC_INFORMATION, VirtualQuery},
    },
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

/// Reentrancy guard: set while the VEH handler or panic hook is running. If the logging code itself
/// triggers an exception (or a panic fires mid-handler), the recursive entry sees this flag and
/// returns immediately, preventing an infinite loop of self-inflicted exceptions that would mask
/// the original fault. Cleared after the handler finishes, so genuinely different exceptions on
/// other threads are still logged.
static IN_HANDLER: AtomicBool = AtomicBool::new(false);

/// Tracks the last logged exception (code + faulting address) to deduplicate repeats. If the same
/// instruction faults repeatedly (common when the game's exception handler retries), only the first
/// occurrence is logged in full; subsequent ones are counted.
static LAST_CODE: AtomicU32 = AtomicU32::new(0);
static LAST_ADDR: AtomicU64 = AtomicU64::new(0);
static REPEAT_COUNT: AtomicU32 = AtomicU32::new(0);

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
    // The same re-entrancy guard the VEH handler uses covers this hook, so a fault while logging a
    // panic can't be logged as a fresh masking record.
    std::panic::set_hook(Box::new(|info| {
        if IN_HANDLER.swap(true, Ordering::SeqCst) {
            return;
        }
        let mut line = Line::new();
        line.str("rust panic");
        if let Some(loc) = info.location() {
            line.str(" at ")
                .str(loc.file())
                .str(":")
                .dec(loc.line() as u64);
        }
        // Extract the message without `core::fmt` (which would format the whole `PanicHookInfo`):
        // the payload is a `&str` or `String` for the overwhelming majority of panics.
        let payload = info.payload();
        let msg = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str));
        if let Some(msg) = msg {
            line.str(": ").str(msg);
        }
        line.flush();
        unsafe { log_backtrace() };
        IN_HANDLER.store(false, Ordering::SeqCst);
    }));
    tracing::info!("Crash handler installed");
}

/// A fixed-capacity line builder that formats directly into a stack buffer, with no heap allocation
/// and no `core::fmt` machinery. See the module docs for why the crash handler cannot use
/// `format!`/`write!`. Appends silently truncate once the buffer is full.
struct Line {
    buf: [u8; 512],
    len: usize,
}

impl Line {
    fn new() -> Self {
        Self {
            buf: [0u8; 512],
            len: 0,
        }
    }

    /// Append the bytes of `s`, truncating at capacity.
    fn str(&mut self, s: &str) -> &mut Self {
        let bytes = s.as_bytes();
        let n = bytes.len().min(self.buf.len() - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&bytes[..n]);
        self.len += n;
        self
    }

    /// Append a single byte, dropping it at capacity.
    fn byte(&mut self, b: u8) -> &mut Self {
        if self.len < self.buf.len() {
            self.buf[self.len] = b;
            self.len += 1;
        }
        self
    }

    /// Append `value` as `0x`-prefixed uppercase hex, zero-padded to at least `width` digits.
    fn hex(&mut self, value: u64, width: usize) -> &mut Self {
        self.str("0x");
        let mut digits = [0u8; 16];
        let mut n = 0;
        let mut v = value;
        loop {
            digits[n] = b"0123456789ABCDEF"[(v & 0xf) as usize];
            n += 1;
            v >>= 4;
            if v == 0 {
                break;
            }
        }
        for _ in n..width {
            self.byte(b'0');
        }
        for i in (0..n).rev() {
            self.byte(digits[i]);
        }
        self
    }

    /// Append `value` as decimal.
    fn dec(&mut self, value: u64) -> &mut Self {
        let mut digits = [0u8; 20];
        let mut n = 0;
        let mut v = value;
        loop {
            digits[n] = b'0' + (v % 10) as u8;
            n += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        for i in (0..n).rev() {
            self.byte(digits[i]);
        }
        self
    }

    /// Write the accumulated line (plus a newline) directly to the crash log, bypassing `tracing`.
    /// Uses `try_lock` so a reentrant call skips the write instead of deadlocking.
    fn flush(&mut self) {
        self.byte(b'\n');
        if let Some(mut guard) = CRASH_LOG.try_lock()
            && let Some(file) = guard.as_mut()
        {
            let _ = file.write_all(&self.buf[..self.len]);
        }
    }
}

unsafe extern "system" fn handler(info: *mut EXCEPTION_POINTERS) -> i32 {
    if IN_HANDLER.swap(true, Ordering::SeqCst) {
        return EXCEPTION_CONTINUE_SEARCH;
    }
    unsafe {
        if let Some(ep) = info.as_ref()
            && let Some(rec) = ep.ExceptionRecord.as_ref()
            && FATAL_CODES.contains(&(rec.ExceptionCode.0 as u32))
        {
            log_record(rec, ep.ContextRecord);
        }
    }
    IN_HANDLER.store(false, Ordering::SeqCst);
    EXCEPTION_CONTINUE_SEARCH
}

/// Append `module=NAME offset=0xNN ` for `addr` if it lies in a loaded module; returns whether it
/// resolved. Uses `VirtualQuery` instead of `GetModuleHandleExW` because the latter takes a
/// `PCWSTR` and Wine's implementation may try to dereference it as a wide string, causing a
/// reentrant access violation inside the crash handler itself. The basename is copied byte-by-byte
/// (lossy ASCII) so it never allocates.
unsafe fn append_module(line: &mut Line, addr: usize) -> bool {
    unsafe {
        let mut mbi = MEMORY_BASIC_INFORMATION::default();
        if VirtualQuery(
            Some(addr as *const std::ffi::c_void),
            &mut mbi,
            std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        ) == 0
        {
            return false;
        }
        let base = mbi.AllocationBase;
        if base.is_null() {
            return false;
        }
        let hmod = HMODULE(base as *mut _);
        let mut buf = [0u16; 260];
        let len = GetModuleFileNameW(Some(hmod), &mut buf) as usize;
        if len == 0 {
            return false;
        }
        // Basename: everything after the last path separator.
        let mut start = 0;
        for (i, &c) in buf[..len].iter().enumerate() {
            if c == b'\\' as u16 || c == b'/' as u16 {
                start = i + 1;
            }
        }
        line.str("module=");
        for &c in &buf[start..len] {
            line.byte(if c < 0x80 { c as u8 } else { b'?' });
        }
        line.str(" offset=")
            .hex(addr.wrapping_sub(hmod.0 as usize) as u64, 1);
        line.byte(b' ');
        true
    }
}

/// Log one frame as `at: module=NAME offset=0xNN addr=0xNN`, falling back to just the raw address
/// when it isn't inside a loaded module.
unsafe fn log_frame(at: &str, addr: usize) {
    unsafe {
        let mut line = Line::new();
        line.str("  ").str(at).str(": ");
        append_module(&mut line, addr);
        line.str("addr=").hex(addr as u64, 16);
        line.flush();
    }
}

/// Capture the current call stack and log each frame, resolved to module+offset.
unsafe fn log_backtrace() {
    unsafe {
        let mut raw = [std::ptr::null_mut::<std::ffi::c_void>(); 48];
        let n = RtlCaptureStackBackTrace(0, &mut raw, None) as usize;
        for (i, f) in raw[..n.min(raw.len())].iter().enumerate() {
            let mut line = Line::new();
            line.str("  bt[");
            if i < 10 {
                line.byte(b'0');
            }
            line.dec(i as u64).str("]: ");
            append_module(&mut line, *f as usize);
            line.str("addr=").hex(*f as usize as u64, 16);
            line.flush();
        }
    }
}

/// Dump key x86-64 registers from the exception context. These are essential for diagnosing the
/// faulting instruction: the write target register, calling-convention arguments, and stack frame
/// pointers narrow down which code path crashed and what it was operating on.
unsafe fn log_context(ctx: *mut CONTEXT) {
    unsafe {
        let Some(ctx) = ctx.as_ref() else {
            Line::new().str("  context: <null>").flush();
            return;
        };
        Line::new()
            .str("  rip=")
            .hex(ctx.Rip, 16)
            .str(" rsp=")
            .hex(ctx.Rsp, 16)
            .str(" rbp=")
            .hex(ctx.Rbp, 16)
            .str(" efl=")
            .hex(ctx.EFlags as u64, 8)
            .flush();
        Line::new()
            .str("  rax=")
            .hex(ctx.Rax, 16)
            .str(" rcx=")
            .hex(ctx.Rcx, 16)
            .str(" rdx=")
            .hex(ctx.Rdx, 16)
            .str(" rbx=")
            .hex(ctx.Rbx, 16)
            .flush();
        Line::new()
            .str("  rsi=")
            .hex(ctx.Rsi, 16)
            .str(" rdi=")
            .hex(ctx.Rdi, 16)
            .str(" r8 =")
            .hex(ctx.R8, 16)
            .str(" r9 =")
            .hex(ctx.R9, 16)
            .flush();
        Line::new()
            .str("  r10=")
            .hex(ctx.R10, 16)
            .str(" r11=")
            .hex(ctx.R11, 16)
            .str(" r12=")
            .hex(ctx.R12, 16)
            .str(" r13=")
            .hex(ctx.R13, 16)
            .flush();
        Line::new()
            .str("  r14=")
            .hex(ctx.R14, 16)
            .str(" r15=")
            .hex(ctx.R15, 16)
            .flush();
    }
}

unsafe fn log_record(rec: &EXCEPTION_RECORD, ctx: *mut CONTEXT) {
    unsafe {
        let code = rec.ExceptionCode.0 as u32;
        let fault_addr = rec.ExceptionAddress as usize as u64;

        // Deduplicate: if this is the same exception at the same instruction, just count it.
        // The first occurrence is logged in full; repeats are summarised every 100.
        if code == LAST_CODE.load(Ordering::Relaxed)
            && fault_addr == LAST_ADDR.load(Ordering::Relaxed)
        {
            let n = REPEAT_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            if n.is_multiple_of(100) {
                Line::new()
                    .str("  ... repeated ")
                    .dec(n as u64)
                    .str(" times")
                    .flush();
            }
            return;
        }

        // New exception. If the previous one repeated, summarise it before logging the new one.
        let prev_repeats = REPEAT_COUNT.swap(0, Ordering::Relaxed);
        if prev_repeats > 0 {
            Line::new()
                .str("  (previous exception repeated ")
                .dec(prev_repeats as u64)
                .str(" times)")
                .flush();
        }
        LAST_CODE.store(code, Ordering::Relaxed);
        LAST_ADDR.store(fault_addr, Ordering::Relaxed);

        let (access_kind, access_addr) = if rec.NumberParameters >= 2 {
            let kind = match rec.ExceptionInformation[0] {
                0 => "read",
                1 => "write",
                8 => "exec",
                _ => "?",
            };
            (kind, rec.ExceptionInformation[1] as u64)
        } else {
            ("n/a", 0)
        };

        Line::new()
            .str("fatal exception: code=")
            .hex(code as u64, 8)
            .str(" access=")
            .str(access_kind)
            .str(" access_addr=")
            .hex(access_addr, 16)
            .flush();

        // Register dump, then the faulting instruction, then the captured stack.
        log_context(ctx);
        log_frame("fault", rec.ExceptionAddress as usize);
        log_backtrace();
    }
}
