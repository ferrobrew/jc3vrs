//! A private Win32 heap behind the global allocator.
//!
//! Rust's default Windows allocator carves from the shared *process* heap -- the same heap Wine's
//! internals, dxvk, and parts of the game allocate from. Two observed failure modes follow from
//! that sharing. First, freed game memory is recycled into payload allocations, so a stale game
//! pointer (a use-after-free on the game/Scaleform side) dereferences into *our* data -- and
//! because Rust data is dense with vtable and function pointers, the resulting indirect calls land
//! as wild jumps inside our image (observed: a Scaleform AS3 call landing mid-instruction in this
//! module's own crash handler). Second, free-list corruption by any party breaks *our* allocation
//! paths (observed: a panic's message formatting dying inside `ntdll`'s heap free-list walk).
//!
//! Allocating from a dedicated `HeapCreate`d heap removes both couplings, and doubles as a
//! diagnostic discriminator: if the wild jumps into our image stop, the recycled memory was ours;
//! if our private heap itself gets corrupted, the smashing write comes from our own code; if
//! nothing changes, the corruption lives entirely in game/Wine memory.

use std::{
    alloc::{GlobalAlloc, Layout},
    ffi::c_void,
    sync::atomic::{AtomicIsize, Ordering},
};

use windows::Win32::{
    Foundation::HANDLE,
    System::Memory::{
        GetProcessHeap, HEAP_FLAGS, HEAP_ZERO_MEMORY, HeapAlloc, HeapCreate, HeapDestroy, HeapFree,
        HeapReAlloc,
    },
};

#[global_allocator]
static ALLOCATOR: PrivateHeap = PrivateHeap;

/// The payload's global allocator: every Rust allocation goes to a dedicated private heap instead
/// of the shared process heap. See the module docs for why.
struct PrivateHeap;

// SAFETY: allocation, deallocation, and reallocation all resolve to the same Win32 heap (the
// handle is latched once in `heap`), the heap is created serialized (no `HEAP_NO_SERIALIZE`), and
// the over-aligned path preserves the raw pointer for `dealloc` in a header slot that the layout
// arithmetic keeps inside the allocation.
unsafe impl GlobalAlloc for PrivateHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { allocate(layout, HEAP_FLAGS(0)) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe { allocate(layout, HEAP_ZERO_MEMORY) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe {
            let raw = if layout.align() <= MIN_ALIGN {
                ptr
            } else {
                (ptr as *mut *mut u8).sub(1).read()
            };
            let _ = HeapFree(heap(), HEAP_FLAGS(0), Some(raw as *const c_void));
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe {
            if layout.align() <= MIN_ALIGN {
                HeapReAlloc(heap(), HEAP_FLAGS(0), Some(ptr as *const c_void), new_size) as *mut u8
            } else {
                // The grow-copy-free path for over-aligned blocks (`HeapReAlloc` only preserves
                // the heap's own alignment), mirroring std's Windows allocator.
                let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
                let new_ptr = allocate(new_layout, HEAP_FLAGS(0));
                if !new_ptr.is_null() {
                    std::ptr::copy_nonoverlapping(ptr, new_ptr, layout.size().min(new_size));
                    self.dealloc(ptr, layout);
                }
                new_ptr
            }
        }
    }
}

/// `HeapAlloc` guarantees `MEMORY_ALLOCATION_ALIGNMENT` -- 16 bytes on x64. Larger alignments are
/// produced by over-allocating in [`allocate`].
const MIN_ALIGN: usize = 16;

/// Allocate `layout` from the private heap. Alignments above [`MIN_ALIGN`] over-allocate by the
/// alignment and store the raw pointer in a header slot directly before the aligned block for
/// [`GlobalAlloc::dealloc`] to recover; `HeapAlloc`'s own 16-byte alignment makes the align-up
/// offset a multiple of 16, so the 8-byte slot never reaches back past the raw start.
unsafe fn allocate(layout: Layout, flags: HEAP_FLAGS) -> *mut u8 {
    unsafe {
        if layout.align() <= MIN_ALIGN {
            return HeapAlloc(heap(), flags, layout.size()) as *mut u8;
        }
        let Some(size) = layout.size().checked_add(layout.align()) else {
            return std::ptr::null_mut();
        };
        let raw = HeapAlloc(heap(), flags, size) as *mut u8;
        if raw.is_null() {
            return raw;
        }
        let offset = layout.align() - (raw as usize & (layout.align() - 1));
        let aligned = raw.add(offset);
        (aligned as *mut *mut u8).sub(1).write(raw);
        aligned
    }
}

/// The latched heap handle; `0` until the first allocation creates it.
static HEAP: AtomicIsize = AtomicIsize::new(0);

/// The private heap, created (growable, serialized) on the first allocation. Racing first
/// allocations may each create a heap; the compare-exchange losers destroy theirs and adopt the
/// winner's. A `HeapCreate` failure falls back to the process heap so allocation never becomes
/// impossible -- the isolation is a hardening measure, not a correctness requirement.
fn heap() -> HANDLE {
    let existing = HEAP.load(Ordering::Acquire);
    if existing != 0 {
        return HANDLE(existing as *mut c_void);
    }
    let private = unsafe { HeapCreate(HEAP_FLAGS(0), 0, 0) }.ok();
    let candidate = private
        .unwrap_or_else(|| unsafe { GetProcessHeap() }.unwrap_or(HANDLE(std::ptr::null_mut())));
    match HEAP.compare_exchange(0, candidate.0 as isize, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => candidate,
        Err(winner) => {
            if let Some(private) = private {
                // SAFETY: this heap lost the race, so no allocation was ever served from it.
                unsafe {
                    let _ = HeapDestroy(private);
                }
            }
            HANDLE(winner as *mut c_void)
        }
    }
}
