//! seL4 Root Task — entry point and test runner for x86_64.

#![no_std]
#![no_main]
#![allow(non_snake_case)]
#![feature(alloc_error_handler)]
#![feature(thread_local)]
#![allow(internal_features)]

extern crate alloc;

mod allocator;
mod benchmark;
mod print;
mod slot;

use core::panic::PanicInfo;
use sel4_sys::*;

use allocator::BumpAllocator;
use slot::{SlotManager, SLOT_MANAGER};
use print::put_u64;

#[global_allocator]
static GLOBAL_ALLOC: BumpAllocator = BumpAllocator;

// Include entry point assembly (entry.S).
core::arch::global_asm!(include_str!("entry.S"));

// ---------------------------------------------------------------------------
// Rust entry point
// ---------------------------------------------------------------------------

#[unsafe(export_name = "sel4_runtime_rust_entry")]
unsafe extern "C" fn rust_entry(bi_frame_vptr: usize) -> ! {
    init_ipc_buffer(bi_frame_vptr);
    main(bi_frame_vptr);
    loop {
        core::hint::spin_loop();
    }
}

fn init_ipc_buffer(bi_frame_vptr: usize) {
    unsafe {
        let ipc_buf_ptr = (bi_frame_vptr - 4096) as *mut IpcBuffer;
        let ipc_buf = &mut *ipc_buf_ptr;
        ipc_buf.set_receive_slot(init_slots::CNODE, 0, 64);
        set_ipc_buffer(ipc_buf);
    }
}

// ---------------------------------------------------------------------------
// Panic handler
// ---------------------------------------------------------------------------

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    seL4_DebugPutString("\n\n=== PANIC ===\n");
    if let Some(location) = info.location() {
        seL4_DebugPutString("Location: ");
        seL4_DebugPutString(location.file());
        seL4_DebugPutChar(b':');
        put_u64(location.line() as u64);
        seL4_DebugPutChar(b'\n');
    }
    if info.message().as_str().is_some() {
        seL4_DebugPutString("Message: (see panic info above)\n");
    }
    seL4_DebugPutString("System halted.\n");
    loop {
        core::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// Main application
// ---------------------------------------------------------------------------

fn main(bi_frame_vptr: usize) {
    let bi = unsafe { BootInfo::from_raw(bi_frame_vptr as *const BootInfoRaw) };
    let untyped_start = bi.untyped_start();
    let empty_region = bi.empty();

    seL4_DebugPutString("[main] empty slots: ");
    put_u64(empty_region.start as u64);
    seL4_DebugPutString("..");
    put_u64(empty_region.end as u64);
    seL4_DebugPutString(" untyped: ");
    put_u64(untyped_start as u64);
    seL4_DebugPutChar(b'\n');

    // Reinitialize SLOT_MANAGER with the actual empty slot range from bootinfo.
    {
        let mut sm = SLOT_MANAGER.lock();
        *sm = SlotManager::new(empty_region.start, empty_region.end);
    }

    seL4_DebugPutString("\n========================================\n");
    seL4_DebugPutString("  rel4-linux-kit -- seL4 x86_64 Tests\n");
    seL4_DebugPutString("========================================\n\n");

    let (passed, failed) = sel4_sys::tests::run_sel4_sys_tests();

    seL4_DebugPutString("\n----------------------------------------\n");
    seL4_DebugPutString("  Results: ");
    put_u64(passed as u64);
    seL4_DebugPutString(" passed, ");
    put_u64(failed as u64);
    seL4_DebugPutString(" failed");
    if failed > 0 {
        seL4_DebugPutString(" (SOME TESTS FAILED)\n");
    } else {
        seL4_DebugPutString(" (all tests passed)\n");
    }
    seL4_DebugPutString("----------------------------------------\n\n");

    print_system_info();

    // Run multi-threaded IPC performance benchmark.
    benchmark::run(&bi);

    seL4_DebugPutString("\nRoot task completed successfully.\n");
    seL4_DebugPutString("Shutting down.\n\n");

    // Trigger QEMU isa-debug-exit via I/O port 0x501.
    let io_slot = { SLOT_MANAGER.lock().alloc().unwrap() };
    let _ = seL4_X86_IOPortControl_Issue(7, 0x501, 0x502, 2, io_slot, 64);
    let _ = seL4_X86_IOPort_Out16(io_slot, 0x501, 0x0001);

    seL4_DebugHalt();
    loop { core::hint::spin_loop(); }
}

fn print_system_info() {
    seL4_DebugPutString("=== System Information ===\n");
    seL4_DebugPutString("  Architecture: x86_64\n");

    let sm = SLOT_MANAGER.lock();
    seL4_DebugPutString("  Free capability slots: ");
    put_u64(sm.available() as u64);
    seL4_DebugPutString("\n");

    seL4_DebugPutString("  Page size: 4096 bytes\n");
    seL4_DebugPutString("  Heap size: ");
    put_u64(allocator::heap_size() as u64);
    seL4_DebugPutString(" bytes\n");
    seL4_DebugPutString("  Stack base: (in .bss)\n");
    seL4_DebugPutString("  IPC buffer size: ");
    put_u64(IPC_BUFFER_SIZE as u64);
    seL4_DebugPutString(" bytes\n");

    seL4_DebugPutString("===========================\n");
}
