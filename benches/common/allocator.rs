use std::alloc::{GlobalAlloc, Layout, System};

// Use the default realloc (allocate/copy/free) to avoid variable in-place growth.
#[global_allocator]
static ALLOCATOR: MovingAllocator = MovingAllocator;

struct MovingAllocator;

// SAFETY: All operations delegate to the same system allocator.
unsafe impl GlobalAlloc for MovingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
