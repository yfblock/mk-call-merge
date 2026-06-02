//! Simple bump allocator for the root task heap.

const HEAP_SIZE: usize = 0x10_0000; // 1 MiB
static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];
static mut HEAP_OFFSET: usize = 0;

#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    panic!(
        "memory allocation failed: size={}, align={}",
        layout.size(),
        layout.align()
    );
}

pub struct BumpAllocator;

unsafe impl core::alloc::GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        unsafe {
            let offset = HEAP_OFFSET;
            let align = layout.align();
            let aligned = (offset + align - 1) & !(align - 1);
            let new_offset = aligned + layout.size();
            if new_offset > HEAP_SIZE {
                core::ptr::null_mut()
            } else {
                HEAP_OFFSET = new_offset;
                let base: *mut u8 = core::ptr::addr_of_mut!(HEAP).cast::<u8>();
                base.add(aligned)
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: core::alloc::Layout) {
        // Bump allocator does not support deallocation.
    }
}

pub fn heap_size() -> usize {
    HEAP_SIZE
}
