//! LCL - Linux Compatible Layer entry point

#![no_std]
#![no_main]
#![allow(unused)]

extern crate alloc;

use alloc::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};
use sel4_sys::*;

// Bump allocator
const HEAP_SIZE: usize = 4 * 1024 * 1024; // 4MB
static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];
static HEAP_POS: AtomicUsize = AtomicUsize::new(0);

struct BumpAllocator;

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pos = HEAP_POS.load(Ordering::Relaxed);
        let aligned = (pos + layout.align() - 1) & !(layout.align() - 1);
        let new_pos = aligned + layout.size();
        if new_pos > HEAP_SIZE {
            core::ptr::null_mut()
        } else {
            HEAP_POS.store(new_pos, Ordering::Relaxed);
            let ptr = core::ptr::addr_of_mut!(HEAP);
            unsafe { (*ptr).as_mut_ptr().add(aligned) }
        }
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator;

core::arch::global_asm!(include_str!("entry.S"));

#[unsafe(export_name = "rust_entry")]
extern "C" fn rust_entry() -> ! {
    seL4_DebugPutString("[lcl] Linux Compatible Layer started\n");

    // TODO: Initialize filesystem, devices, exception handler
    // TODO: Load and run ELF binaries

    seL4_DebugPutString("[lcl] Initialization complete\n");
    seL4_DebugPutString("[lcl] Ready to run Linux binaries\n");

    // Halt for now
    seL4_DebugPutString("[lcl] Test completed.\n");
    seL4_DebugHalt();

    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    seL4_DebugPutString("[lcl] PANIC\n");
    loop {
        core::hint::spin_loop();
    }
}
