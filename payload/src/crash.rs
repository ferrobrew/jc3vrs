//! First-chance crash instrumentation.
//!
//! A vectored exception handler logs the faulting address, a register dump, a module-resolved
//! backtrace of the faulting thread, and a backtrace of every other thread the moment a fatal
//! exception is *raised* -- before any handler unwinds. This covers the case where the game catches a
//! fault itself and turns it into a clean exit: Wine prints no backtrace and the window just
//! vanishes, but the record still lands in `jc3vrs.log`. A panic hook does the same for Rust panics,
//! which don't raise an SEH exception and so are invisible to the VEH handler. Each address is
//! resolved to its containing module + offset (`module+0xoff`) via `VirtualQuery`, which works under
//! Wine where `std::backtrace` usually can't symbolize.
//!
//! **The whole logging path is allocation-free, lock-free, and uses no `core::fmt` or `std::io`.**
//! Everything is formatted manually into a fixed [`Line`] stack buffer and written with a direct
//! `WriteFile` syscall to a raw `HANDLE` opened once at [`install`]. This is critical: when the
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
//! handler retries the faulting instruction.

use std::{
    os::windows::ffi::OsStrExt,
    sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, AtomicUsize, Ordering},
};

use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, HMODULE},
        Storage::FileSystem::{
            CreateFileW, FILE_APPEND_DATA, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ,
            FILE_SHARE_WRITE, OPEN_ALWAYS, WriteFile,
        },
        System::{
            Diagnostics::{
                Debug::{
                    AddVectoredExceptionHandler, CONTEXT, CONTEXT_FLAGS, EXCEPTION_POINTERS,
                    EXCEPTION_RECORD, GetThreadContext, RtlCaptureStackBackTrace,
                },
                ToolHelp::{
                    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                    Thread32Next,
                },
            },
            LibraryLoader::GetModuleFileNameW,
            Memory::{MEMORY_BASIC_INFORMATION, VirtualQuery},
            Threading::{
                GetCurrentProcessId, GetCurrentThreadId, OpenThread, ResumeThread, SuspendThread,
                THREAD_GET_CONTEXT, THREAD_SUSPEND_RESUME,
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

/// `CONTEXT_AMD64 | CONTEXT_CONTROL | CONTEXT_INTEGER`: the register groups [`dump_thread`] needs
/// (`Rip`/`Rsp`/`Rbp` and the general-purpose registers).
const CONTEXT_AMD64_CONTROL_INTEGER: u32 = 0x0010_0003;
/// The page-protection bits that mark executable memory; a stack value pointing at executable code is
/// a probable return address.
const PAGE_EXECUTE_ANY: u32 = 0xF0;
/// Stop dumping after this many threads, so a runaway thread count can't flood the log.
const MAX_THREADS: usize = 96;
/// Stack words scanned per thread for return addresses (8 KiB).
const STACK_SCAN_WORDS: usize = 1024;
/// Probable frames logged per thread.
const MAX_FRAMES_PER_THREAD: usize = 24;

/// Off-stack scratch for [`dump_thread`]: the suspended thread's `CONTEXT` (~1.2 KiB) and the
/// stack-scan array (8 KiB). Kept out of the stack frame because the handler may run on a nearly
/// exhausted stack -- when the original fault is a stack overflow, a multi-KiB local in the handler
/// blows the guard page and faults the handler itself, masking the real crash. `dump_other_threads`
/// calls `dump_thread` sequentially under the [`IN_HANDLER`] guard, so a single shared static is safe.
#[repr(C, align(16))]
struct ThreadScratch {
    ctx: CONTEXT,
    stack: [usize; STACK_SCAN_WORDS],
}
struct ThreadScratchCell(std::cell::UnsafeCell<ThreadScratch>);
// SAFETY: only ever accessed from `dump_thread`, which runs single-threaded under the `IN_HANDLER`
// re-entrancy guard; there is no concurrent access to synchronise.
unsafe impl Sync for ThreadScratchCell {}
static THREAD_SCRATCH: ThreadScratchCell = ThreadScratchCell(std::cell::UnsafeCell::new(
    // SAFETY: `CONTEXT` and `[usize; N]` are plain POD with no validity requirement violated by an
    // all-zero bit pattern, so a zeroed initializer is sound and gives a `const` for the static.
    unsafe { std::mem::zeroed() },
));

/// The raw file handle for crash-handler writes, opened once at [`install`] and never closed (the
/// process is dying). `0` means "not opened". Stored as an `isize` so it lives in an atomic without a
/// lock -- the handler must not touch `parking_lot` or `std::io`.
static CRASH_LOG: AtomicIsize = AtomicIsize::new(0);

/// Reentrancy guard: set while the VEH handler or panic hook is running. If the logging code itself
/// triggers an exception (or a panic fires mid-handler), the recursive entry sees this flag and
/// returns immediately, preventing an infinite loop of self-inflicted exceptions that would mask the
/// original fault. Cleared after the handler finishes, so genuinely different exceptions on other
/// threads are still logged.
static IN_HANDLER: AtomicBool = AtomicBool::new(false);

/// Tracks the last logged exception (code + faulting address) to deduplicate repeats. If the same
/// instruction faults repeatedly (common when the game's exception handler retries), only the first
/// occurrence is logged in full; subsequent ones are counted.
static LAST_CODE: AtomicU32 = AtomicU32::new(0);
static LAST_ADDR: AtomicU64 = AtomicU64::new(0);
static REPEAT_COUNT: AtomicU32 = AtomicU32::new(0);

/// A frame-loop milestone, recorded into the [`BREADCRUMBS`] ring by [`mark`]. When a crash's stack is
/// unreliable (COMDAT-folded generics, a smashed stack, an unwind that can't cross the exception
/// frame), the ordered ring of recent phases still says *where in the frame* execution was -- which the
/// backtrace alone often cannot. Reading it in the handler is a plain array load, so it can never
/// itself fault.
#[derive(Clone, Copy)]
#[repr(u32)]
pub enum Phase {
    UpdateRenderEnter = 1,
    OriginalUpdateRender,
    Eye0Snapshot,
    Eye0Draw,
    Eye0Drain,
    Eye0Post,
    BetweenEyesRestore,
    Eye1Draw,
    Eye1Drain,
    Eye1Post,
    Present,
    NonStereoDraw,
    FrameEnd,
}

const BREADCRUMB_COUNT: usize = 24;
static BREADCRUMBS: [AtomicU32; BREADCRUMB_COUNT] = [const { AtomicU32::new(0) }; BREADCRUMB_COUNT];
static BREADCRUMB_POS: AtomicUsize = AtomicUsize::new(0);

/// Record a frame-loop milestone. A single relaxed store into a ring -- no I/O, no lock, cheap enough
/// to call on every phase transition each frame. The handler dumps the ring on a crash.
pub fn mark(phase: Phase) {
    let pos = BREADCRUMB_POS.fetch_add(1, Ordering::Relaxed);
    BREADCRUMBS[pos % BREADCRUMB_COUNT].store(phase as u32, Ordering::Relaxed);
}

fn phase_name(code: u32) -> &'static str {
    match code {
        1 => "UpdateRenderEnter",
        2 => "OriginalUpdateRender",
        3 => "Eye0Snapshot",
        4 => "Eye0Draw",
        5 => "Eye0Drain",
        6 => "Eye0Post",
        7 => "BetweenEyesRestore",
        8 => "Eye1Draw",
        9 => "Eye1Drain",
        10 => "Eye1Post",
        11 => "Present",
        12 => "NonStereoDraw",
        13 => "FrameEnd",
        _ => "?",
    }
}

/// Dump the breadcrumb ring, newest first (so truncation drops the oldest, never the crash point).
fn log_breadcrumbs() {
    let pos = BREADCRUMB_POS.load(Ordering::Relaxed);
    if pos == 0 {
        return;
    }
    let count = pos.min(BREADCRUMB_COUNT);
    let mut line = Line::new();
    line.str("recent phases (newest first): ");
    for i in 0..count {
        let code = BREADCRUMBS[(pos - 1 - i) % BREADCRUMB_COUNT].load(Ordering::Relaxed);
        line.str(phase_name(code));
        if i + 1 < count {
            line.str(" <- ");
        }
    }
    line.flush();
}

pub fn install() {
    // Open the log file with a raw handle, sharing the same `jc3vrs.log` the tracing subscriber
    // writes to. FILE_APPEND_DATA makes every WriteFile append, and the share flags let the tracing
    // subscriber keep its own handle open.
    if let Some(path) = crate::module::get_path()
        .as_ref()
        .and_then(|path| path.parent())
        .map(|parent| parent.join("jc3vrs.log"))
    {
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
        }
    }

    unsafe { AddVectoredExceptionHandler(1, Some(handler)) };
    // Rust panics unwind/abort instead of raising an SEH exception, so the VEH handler above never
    // sees them. Log the message + a backtrace ourselves before the process dies. The same
    // re-entrancy guard the VEH handler uses covers this hook.
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

    /// Append a newline and write the line straight to the crash log with a single `WriteFile`. No
    /// heap, no mutex, no `std::io` -- just the stored handle and the stack buffer.
    fn flush(&mut self) {
        self.byte(b'\n');
        let raw = CRASH_LOG.load(Ordering::Relaxed);
        if raw == 0 {
            return;
        }
        let handle = HANDLE(raw as *mut std::ffi::c_void);
        // SAFETY: `handle` is the append-mode log handle from `install`; `self.buf[..self.len]` is a
        // valid slice. A failed write is ignored -- there is nowhere better to report it.
        unsafe {
            let _ = WriteFile(handle, Some(&self.buf[..self.len]), None, None);
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
/// resolved. Uses `VirtualQuery` instead of `GetModuleHandleExW` because the latter takes a `PCWSTR`
/// and Wine's implementation may try to dereference it as a wide string, causing a reentrant access
/// violation inside the crash handler itself. The basename is copied byte-by-byte (lossy ASCII) so it
/// never allocates.
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
        let mut buf = [0u16; 260];
        let len = GetModuleFileNameW(Some(HMODULE(base)), &mut buf) as usize;
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
            .hex(addr.wrapping_sub(base as usize) as u64, 1);
        line.byte(b' ');
        true
    }
}

/// Whether `addr` points into committed, executable memory -- i.e. a value on the stack that is a
/// probable return address rather than data.
unsafe fn is_executable(addr: usize) -> bool {
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
        mbi.Protect.0 & PAGE_EXECUTE_ANY != 0
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

/// Log a heuristic backtrace of the *faulting* thread by scanning its stack upward from `rsp` for values
/// that point into executable memory (probable return addresses). Unlike `RtlCaptureStackBackTrace`, it
/// never invokes the unwinder, so it cannot fault on a smashed or foreign-code stack -- exactly the case
/// where the real crash most needs a backtrace and Wine's unwinder itself faults, masking it. `rsp`
/// comes from the exception `CONTEXT`. Reuses the shared `THREAD_SCRATCH` stack buffer, which is safe:
/// the handler is serialized by `IN_HANDLER`, and this runs before `dump_other_threads` reuses it.
unsafe fn log_faulting_stack(rsp: u64) {
    unsafe {
        if rsp == 0 {
            return;
        }
        let stack = &mut (*THREAD_SCRATCH.0.get()).stack;
        let mut mbi = MEMORY_BASIC_INFORMATION::default();
        if VirtualQuery(
            Some(rsp as *const std::ffi::c_void),
            &mut mbi,
            std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        ) == 0
        {
            return;
        }
        let region_end = mbi.BaseAddress as usize + mbi.RegionSize;
        let words = (region_end.saturating_sub(rsp as usize) / 8).min(STACK_SCAN_WORDS);
        for (i, slot) in stack[..words].iter_mut().enumerate() {
            *slot = *((rsp as usize + i * 8) as *const usize);
        }
        let mut frames = 0usize;
        for &value in &stack[..words] {
            if frames >= MAX_FRAMES_PER_THREAD {
                break;
            }
            if is_executable(value) {
                let mut line = Line::new();
                line.str("  bt[");
                if frames < 10 {
                    line.byte(b'0');
                }
                line.dec(frames as u64).str("]: ");
                if append_module(&mut line, value) {
                    line.str("addr=").hex(value as u64, 16);
                    line.flush();
                    frames += 1;
                }
            }
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

        // Register dump, the faulting instruction, and the faulting thread's stack first -- the
        // essential record. The other-thread dump (riskier: it suspends threads) runs last, so a
        // fault there can't lose the primary information.
        log_context(ctx);
        log_frame("fault", rec.ExceptionAddress as usize);
        // Scan the faulting thread's own stack heuristically rather than calling
        // RtlCaptureStackBackTrace on it: under Wine the unwinder walks a foreign/smashed stack and
        // faults itself, masking the very crash we are reporting (dump_other_threads excludes the
        // current thread, so this is the only place the faulting stack is recovered).
        log_faulting_stack(ctx.as_ref().map_or(0, |c| c.Rsp));
        log_breadcrumbs();
        dump_other_threads();
    }
}

/// Walk every other thread in the process and log its instruction pointer plus a heuristic backtrace
/// (stack values that point at executable code). Runs after the primary record so any fault here
/// leaves that record intact. Resolves modules only after each thread is resumed, so a thread holding
/// the loader lock cannot deadlock `GetModuleFileNameW`.
unsafe fn dump_other_threads() {
    unsafe {
        let pid = GetCurrentProcessId();
        let current = GetCurrentThreadId();
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else {
            return;
        };

        Line::new().str("-- other threads --").flush();

        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let mut dumped = 0usize;
        if Thread32First(snapshot, &mut entry).is_ok() {
            loop {
                if entry.th32OwnerProcessID == pid
                    && entry.th32ThreadID != current
                    && dumped < MAX_THREADS
                {
                    dump_thread(entry.th32ThreadID);
                    dumped += 1;
                }
                if Thread32Next(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }
}

/// Suspend one thread, capture its register context and a bounded copy of its stack, resume it, then
/// log its `rip` and the probable return addresses found on the stack.
unsafe fn dump_thread(tid: u32) {
    unsafe {
        let Ok(handle) = OpenThread(THREAD_GET_CONTEXT | THREAD_SUSPEND_RESUME, false, tid) else {
            return;
        };

        // The CONTEXT (16-byte aligned for GetThreadContext) and the stack-scan array live in a shared
        // static rather than on the stack -- see THREAD_SCRATCH for why. Reset only the fields used.
        let scratch = &mut *THREAD_SCRATCH.0.get();
        scratch.ctx.ContextFlags = CONTEXT_FLAGS(CONTEXT_AMD64_CONTROL_INTEGER);
        let stack = &mut scratch.stack;
        let mut stack_words = 0usize;
        let mut rip = 0u64;
        let mut rsp = 0u64;

        let suspended = SuspendThread(handle) != u32::MAX;
        if suspended && GetThreadContext(handle, &mut scratch.ctx).is_ok() {
            rip = scratch.ctx.Rip;
            rsp = scratch.ctx.Rsp;
            // Copy a bounded, in-bounds slice of the stack while the thread is frozen. VirtualQuery
            // bounds the read to the committed region so the copy never faults.
            let mut mbi = MEMORY_BASIC_INFORMATION::default();
            if rsp != 0
                && VirtualQuery(
                    Some(rsp as *const std::ffi::c_void),
                    &mut mbi,
                    std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
                ) != 0
            {
                let region_end = mbi.BaseAddress as usize + mbi.RegionSize;
                let available = region_end.saturating_sub(rsp as usize) / 8;
                stack_words = available.min(STACK_SCAN_WORDS);
                for (i, slot) in stack[..stack_words].iter_mut().enumerate() {
                    *slot = *((rsp as usize + i * 8) as *const usize);
                }
            }
        }
        if suspended {
            ResumeThread(handle);
        }
        let _ = CloseHandle(handle);

        // Thread resumed: now resolve and log (GetModuleFileNameW takes the loader lock, which a
        // suspended thread might hold).
        let mut line = Line::new();
        line.str("thread ").dec(tid as u64).str(": rip=");
        append_module(&mut line, rip as usize);
        line.str("addr=").hex(rip, 16).str(" rsp=").hex(rsp, 16);
        line.flush();

        let mut frames = 0usize;
        for &value in &stack[..stack_words] {
            if frames >= MAX_FRAMES_PER_THREAD {
                break;
            }
            if is_executable(value) {
                let mut frame = Line::new();
                frame.str("  ");
                if append_module(&mut frame, value) {
                    frame.str("addr=").hex(value as u64, 16);
                    frame.flush();
                    frames += 1;
                }
            }
        }
    }
}
