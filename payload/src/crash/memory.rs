//! `VirtualQuery`-based memory probes and module resolution. Everything here is allocation-free and
//! touches no loader-owned strings, so it is safe to call from the crash handler.

use std::{
    os::windows::ffi::OsStrExt,
    sync::atomic::{AtomicUsize, Ordering},
};

use windows::{
    Win32::{
        Foundation::HMODULE,
        System::{
            LibraryLoader::{GetModuleFileNameW, GetModuleHandleW},
            Memory::{MEMORY_BASIC_INFORMATION, VirtualQuery},
        },
    },
    core::PCWSTR,
};

use crate::crash::line::Line;

/// Resolve the allocation bases of the probe-host modules, for
/// [`faulting_module_is_probe_host`].
pub(super) fn resolve_probe_host_bases() {
    for (slot, name) in PROBE_HOST_BASES
        .iter()
        .zip(["kernelbase.dll", "ucrtbase.dll", "ntdll.dll"])
    {
        let wide: Vec<u16> = std::ffi::OsStr::new(name)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: `wide` is a null-terminated UTF-16 module name.
        if let Ok(module) = unsafe { GetModuleHandleW(PCWSTR(wide.as_ptr())) } {
            slot.store(module.0 as usize, Ordering::Relaxed);
        }
    }
}

/// Whether `addr` lies inside one of the system modules whose functions carry their own SEH
/// probes (`lstrlen`, `memcpy` wrappers, dispatch internals): kernelbase, ucrtbase, and ntdll.
/// Resolved by allocation base so no names are touched in the handler.
pub(super) fn faulting_module_is_probe_host(addr: usize) -> bool {
    if addr == 0 {
        return false;
    }
    let mut mbi = MEMORY_BASIC_INFORMATION::default();
    // SAFETY: VirtualQuery fills a plain struct; a zero return means unmapped.
    let len = unsafe {
        VirtualQuery(
            Some(addr as *const std::ffi::c_void),
            &mut mbi,
            std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    if len == 0 {
        return false;
    }
    let base = mbi.AllocationBase as usize;
    base != 0
        && PROBE_HOST_BASES
            .iter()
            .any(|b| b.load(Ordering::Relaxed) == base)
}

/// Whether `[addr, addr + len)` starts in committed, readable memory. A `VirtualQuery` probe, so it
/// is allocation-free and safe to call from the handler; it cannot fully close the race against a
/// concurrent unmap, but it filters the already-dead records observed under Wine.
pub(super) unsafe fn readable(addr: usize, len: usize) -> bool {
    if addr == 0 {
        return false;
    }
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
        const READABLE_PROTECTIONS: u32 = 0x02 | 0x04 | 0x08 | 0x20 | 0x40 | 0x80;
        const PAGE_GUARD_BIT: u32 = 0x100;
        let protect = mbi.Protect.0;
        mbi.State.0 == 0x1000 // MEM_COMMIT.
            && (protect & READABLE_PROTECTIONS) != 0
            // A guard page is "readable" by protection bits but faults on touch (stack growth).
            && (protect & PAGE_GUARD_BIT) == 0
            && (protect & 0x100) == 0 // PAGE_GUARD.
            && addr + len <= mbi.BaseAddress as usize + mbi.RegionSize
    }
}

/// Whether `addr` points into committed, executable memory -- i.e. a value on the stack that is a
/// probable return address rather than data.
pub(super) unsafe fn is_executable(addr: usize) -> bool {
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

/// Append `module=NAME offset=0xNN ` for `addr` if it lies in a loaded module; returns whether it
/// resolved. Uses `VirtualQuery` instead of `GetModuleHandleExW` because the latter takes a `PCWSTR`
/// and Wine's implementation may try to dereference it as a wide string, causing a reentrant access
/// violation inside the crash handler itself. The basename is copied byte-by-byte (lossy ASCII) so it
/// never allocates.
pub(super) unsafe fn append_module(line: &mut Line, addr: usize) -> bool {
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

/// The page-protection bits that mark executable memory; a stack value pointing at executable code is
/// a probable return address.
const PAGE_EXECUTE_ANY: u32 = 0xF0;

/// The allocation bases of kernelbase, ucrtbase, and ntdll, resolved at [`crate::crash::install`].
static PROBE_HOST_BASES: [AtomicUsize; 3] = [const { AtomicUsize::new(0) }; 3];
