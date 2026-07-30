//! Stack scanning and thread enumeration: the heuristic backtrace of the faulting thread, and the
//! suspend/capture/resume dump of every other thread in the process.

use windows::Win32::{
    Foundation::CloseHandle,
    System::{
        Diagnostics::{
            Debug::{CONTEXT, CONTEXT_FLAGS, GetThreadContext},
            ToolHelp::{
                CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                Thread32Next,
            },
        },
        Memory::{MEMORY_BASIC_INFORMATION, VirtualQuery},
        Threading::{
            GetCurrentProcessId, GetCurrentThreadId, OpenThread, ResumeThread, SuspendThread,
            THREAD_GET_CONTEXT, THREAD_SUSPEND_RESUME,
        },
    },
};

use crate::crash::{
    line::Line,
    memory::{append_module, is_executable, readable},
};

/// Log a heuristic backtrace of the *faulting* thread by scanning its stack upward from `rsp` for values
/// that point into executable memory (probable return addresses). Unlike `RtlCaptureStackBackTrace`, it
/// never invokes the unwinder, so it cannot fault on a smashed or foreign-code stack -- exactly the case
/// where the real crash most needs a backtrace and Wine's unwinder itself faults, masking it. `rsp`
/// comes from the exception `CONTEXT`. Reuses the shared `THREAD_SCRATCH` stack buffer, which is safe:
/// the handler is serialized by `IN_HANDLER`, and this runs before `dump_other_threads` reuses it.
pub(super) unsafe fn log_faulting_stack(rsp: u64) {
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
        // The region must be committed and readable, not merely present: a dying thread's stack
        // can be reserved-but-decommitted, and scanning it faulted the handler itself (observed
        // in-game as an access violation at this function mid-record).
        if !readable(rsp as usize, 8) {
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

/// Walk every other thread in the process and log its instruction pointer plus a heuristic backtrace
/// (stack values that point at executable code). Runs after the primary record so any fault here
/// leaves that record intact. Resolves modules only after each thread is resumed, so a thread holding
/// the loader lock cannot deadlock `GetModuleFileNameW`.
pub(super) unsafe fn dump_other_threads() {
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

/// `CONTEXT_AMD64 | CONTEXT_CONTROL | CONTEXT_INTEGER`: the register groups [`dump_thread`] needs
/// (`Rip`/`Rsp`/`Rbp` and the general-purpose registers).
const CONTEXT_AMD64_CONTROL_INTEGER: u32 = 0x0010_0003;
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
/// calls `dump_thread` sequentially under the `IN_HANDLER` guard, so a single shared static is safe.
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
                // Committed and readable, not merely present — see log_faulting_stack.
                && readable(rsp as usize, 8)
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
