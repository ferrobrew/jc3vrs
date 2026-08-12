//! Turning a raised exception into a crash-log record: snapshotting the `EXCEPTION_RECORD`, the
//! dedup and storm throttles that decide how much of a record to write, and the record body itself
//! (fault line, registers, backtraces).

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use windows::Win32::System::{
    Diagnostics::Debug::{CONTEXT, EXCEPTION_POINTERS, EXCEPTION_RECORD, RtlCaptureStackBackTrace},
    SystemInformation::GetTickCount64,
};

use crate::crash::{
    breadcrumbs::log_breadcrumbs,
    line::{Line, stamp},
    memory::{append_module, faulting_module_is_probe_host, readable},
    threads::{dump_other_threads, log_faulting_stack},
};

/// The body of `handler`: snapshot the exception record and log it. Split out so the handler
/// can wrap it in `catch_unwind`.
pub(super) unsafe fn record_exception(info: *mut EXCEPTION_POINTERS) {
    unsafe {
        // Snapshot everything needed from the exception record in one tight window, after probing
        // that the record memory is actually committed and readable. Under Wine, records for
        // exceptions raised on dying worker threads (RPC) have been observed to be torn or already
        // unmapped by the time the handler reads them: a re-read of the code mid-logging returned
        // float bit patterns, and a late dereference faulted inside the handler itself -- an AV in
        // a vectored handler escalates otherwise-handled exception traffic into process death. The
        // probe-then-read window is still racy in principle, but a single read can no longer
        // disagree with itself, and nothing dereferences the record after this block.
        //
        // The reads are volatile because a plain copy is not actually a single read: the record
        // often lives on the faulting thread's own stack, which Wine's recursive exception
        // dispatch rewrites while the handler runs, and the optimizer is free to defer each
        // field's load to its use site -- observed in-game as records whose printed code was
        // stack garbage that could never have passed the FATAL_CODES gate below (the gate's load
        // and the print's load were separate reads of changed memory).
        if let Some(ep) = info.as_ref() {
            let rec_ptr = ep.ExceptionRecord;
            if !rec_ptr.is_null()
                && readable(rec_ptr as usize, std::mem::size_of::<EXCEPTION_RECORD>())
            {
                let record = FaultRecord {
                    code: std::ptr::read_volatile(&raw const (*rec_ptr).ExceptionCode).0 as u32,
                    address: std::ptr::read_volatile(&raw const (*rec_ptr).ExceptionAddress)
                        as usize,
                    parameter_count: std::ptr::read_volatile(
                        &raw const (*rec_ptr).NumberParameters,
                    ),
                    info0: std::ptr::read_volatile(&raw const (*rec_ptr).ExceptionInformation[0]),
                    info1: std::ptr::read_volatile(&raw const (*rec_ptr).ExceptionInformation[1]),
                };
                if FATAL_CODES.contains(&record.code) && !EXCLUDED_CODES.contains(&record.code) {
                    let ctx = ep.ContextRecord;
                    let ctx = if readable(ctx as usize, std::mem::size_of::<CONTEXT>()) {
                        ctx
                    } else {
                        std::ptr::null_mut()
                    };
                    log_record(record, ctx);
                }
            }
        }
    }
}

/// Capture the current call stack and log each frame, resolved to module+offset.
pub(super) unsafe fn log_backtrace() {
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

/// The MSVC C++ exception code: the SEH top-level filter reports any `throw` as this, so it is a
/// deliberate handling path, not a fatal condition to record.
const MSVC_CPP_EXCEPTION_CODE: u32 = 0xE06D7363;

/// `STATUS_GUARD_PAGE_VIOLATION`: the first-chance fault Windows raises when a stack guard page is
/// touched during growth. Benign when handled by the OS; not worth recording as a crash.
const GUARD_PAGE_VIOLATION_CODE: u32 = 0x80000001;

/// Fatal codes worth recording -- skip C++ exceptions ([`MSVC_CPP_EXCEPTION_CODE`]),
/// debug/breakpoint events and benign first-chance ones ([`GUARD_PAGE_VIOLATION_CODE`], not in
/// this list).
const FATAL_CODES: &[u32] = &[
    0xC0000005, // ACCESS_VIOLATION
    0xC000001D, // ILLEGAL_INSTRUCTION
    0xC0000094, // INTEGER_DIVIDE_BY_ZERO
    0xC0000096, // PRIVILEGED_INSTRUCTION
    0xC00000FD, // STACK_OVERFLOW
    // Genuinely process-killing conditions, added after a session died with no record at all:
    // heap corruption is the likely end state of the observed Scaleform exception storms, and
    // fail-fast may bypass handlers on real Windows but is visible under Wine's dispatch.
    0xC0000374, // HEAP_CORRUPTION
    0xC0000409, // STACK_BUFFER_OVERRUN / FAIL_FAST
];

/// Codes that must never be logged as fatal even if present: MSVC C++ exceptions (handled by the
/// C++ runtime) and the benign first-chance guard-page fault. Kept as a separate gate so the
/// exclusion is enforced in code, not just documented.
const EXCLUDED_CODES: &[u32] = &[MSVC_CPP_EXCEPTION_CODE, GUARD_PAGE_VIOLATION_CODE];

/// The fields of an `EXCEPTION_RECORD` the logger uses, copied out by value in `handler` so no
/// code path dereferences the record after its one probed read window.
#[derive(Clone, Copy)]
struct FaultRecord {
    code: u32,
    address: usize,
    parameter_count: u32,
    info0: usize,
    info1: usize,
}

/// Tracks the last logged exception (code + faulting address) to deduplicate repeats. If the same
/// instruction faults repeatedly (common when the game's exception handler retries), only the first
/// occurrence is logged in full; subsequent ones are counted.
static LAST_CODE: AtomicU32 = AtomicU32::new(0);
static LAST_ADDR: AtomicU64 = AtomicU64::new(0);
static REPEAT_COUNT: AtomicU32 = AtomicU32::new(0);

/// Storm throttle: the identical-repeat dedup above is defeated by exception storms that alternate
/// between a handful of faulting addresses (observed: Scaleform probing garbage pointers every
/// ~10 ms, each record writing a full thread dump — megabytes in a second, burying the record that
/// actually killed the process). At most [`STORM_FULL_RECORDS`] records per [`STORM_WINDOW_MS`]
/// window get the full dump; the rest still get their head and fault lines, so the death point
/// stays findable, without the multi-hundred-line body.
static STORM_WINDOW_START_MS: AtomicU64 = AtomicU64::new(0);
static STORM_WINDOW_RECORDS: AtomicU32 = AtomicU32::new(0);

/// Full records allowed per storm window before throttling kicks in.
const STORM_FULL_RECORDS: u32 = 5;
/// The storm-throttle window length in milliseconds.
const STORM_WINDOW_MS: u64 = 1000;

unsafe fn log_record(rec: FaultRecord, ctx: *mut CONTEXT) {
    unsafe {
        let code = rec.code;
        let fault_addr = rec.address as u64;

        // Deduplicate: if this is the same exception at the same instruction, just count it.
        // The first occurrence is logged in full; repeats are summarised every 100.
        if code == LAST_CODE.load(Ordering::Relaxed)
            && fault_addr == LAST_ADDR.load(Ordering::Relaxed)
        {
            let n = REPEAT_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            if n.is_multiple_of(100) {
                let mut line = Line::new();
                line.str("  ... repeated ").dec(n as u64).str(" times, at ");
                stamp(&mut line);
                line.flush();
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

        let (access_kind, access_addr) = if rec.parameter_count >= 2 {
            let kind = match rec.info0 {
                0 => "read",
                1 => "write",
                8 => "exec",
                _ => "?",
            };
            (kind, rec.info1 as u64)
        } else {
            ("n/a", 0)
        };

        // Faults raised inside the system's internally-guarded probe functions (lstrlen-style
        // SEH in kernelbase/ucrtbase/ntdll) are routine first-chance traffic the game survives in
        // storms — and the deep dumps below kept faulting the handler itself mid-storm, each time
        // somewhere new (a scan of a decommitted stack, then formatting with trampled spills).
        // Give them the shallow record only; real crashes fault in game or payload code and keep
        // the full dump.
        let shallow_probe_fault = faulting_module_is_probe_host(rec.address);

        // Storm throttle: cap the number of full dumps per window. The alternating-address storms
        // that motivate this slip past the identical-repeat dedup above.
        let now_ms = GetTickCount64();
        if now_ms.saturating_sub(STORM_WINDOW_START_MS.load(Ordering::Relaxed)) > STORM_WINDOW_MS {
            STORM_WINDOW_START_MS.store(now_ms, Ordering::Relaxed);
            STORM_WINDOW_RECORDS.store(0, Ordering::Relaxed);
        }
        let full_record = STORM_WINDOW_RECORDS.fetch_add(1, Ordering::Relaxed) < STORM_FULL_RECORDS;

        // "first-chance": the VEH sees the exception before any handler; the game (or an
        // internally-guarded probe like lstrlen) frequently handles it and carries on, so a record
        // here does not imply the process died. The record that killed the process is simply the
        // last one that was ever written.
        let mut head = Line::new();
        stamp(&mut head);
        head.str("exception (first-chance): code=")
            .hex(code as u64, 8)
            .str(" access=")
            .str(access_kind)
            .str(" access_addr=")
            .hex(access_addr, 16);
        if !full_record {
            head.str(" (storm: dump throttled)");
        }
        if shallow_probe_fault {
            head.str(" (probe-host fault: dump skipped)");
        }
        head.flush();

        // The fault line is cheap and always written, so throttled records stay attributable.
        log_frame("fault", rec.address);
        if !full_record || shallow_probe_fault {
            return;
        }

        // Register dump and the faulting thread's stack first -- the essential record. The
        // other-thread dump (riskier: it suspends threads) runs last, so a fault there can't lose
        // the primary information.
        log_context(ctx);
        // Scan the faulting thread's own stack heuristically rather than calling
        // RtlCaptureStackBackTrace on it: under Wine the unwinder walks a foreign/smashed stack and
        // faults itself, masking the very crash we are reporting (dump_other_threads excludes the
        // current thread, so this is the only place the faulting stack is recovered).
        log_faulting_stack(ctx.as_ref().map_or(0, |c| c.Rsp));
        log_breadcrumbs();
        dump_other_threads();
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
