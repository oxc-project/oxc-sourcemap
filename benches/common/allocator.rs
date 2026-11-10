use std::alloc::{GlobalAlloc, Layout, System};

// Keep realloc on the allocate/copy/free path: in-place growth depends on
// earlier allocations and can change simulated costs between unrelated runs.
// This allocator is only enabled for CodSpeed benchmark binaries.
// https://codspeed.io/docs/instruments/cpu/reducing-variance
#[global_allocator]
static ALLOCATOR: MovingAllocator = MovingAllocator;

struct MovingAllocator;

// SAFETY: Allocation and deallocation use the same system allocator. The default
// GlobalAlloc::realloc implementation copies through these methods.
unsafe impl GlobalAlloc for MovingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: The caller provides a valid allocation layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: The caller provides a valid allocation layout.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: Allocations from this wrapper come from System with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}
