//! ext4 filesystem service for seL4
//!
//! This service provides file operations via IPC for the Linux Compatible Layer.

#![no_std]
#![no_main]
#![allow(non_snake_case)]
#![allow(dead_code)]

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;

/// Simple bump allocator for the service
struct BumpAllocator {
    heap: UnsafeCell<[u8; 1024 * 1024]>, // 1MB heap
    offset: UnsafeCell<usize>,
}

unsafe impl Sync for BumpAllocator {}

static GLOBAL_ALLOC: BumpAllocator = BumpAllocator {
    heap: UnsafeCell::new([0; 1024 * 1024]),
    offset: UnsafeCell::new(0),
};

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();
        let offset = &mut *self.offset.get();
        let heap = &mut *self.heap.get();

        let aligned_offset = (*offset + align - 1) & !(align - 1);

        if aligned_offset + size > heap.len() {
            core::ptr::null_mut()
        } else {
            *offset = aligned_offset + size;
            heap.as_mut_ptr().add(aligned_offset)
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator doesn't dealloc
    }
}

#[global_allocator]
static ALLOC: BumpAllocator = BumpAllocator {
    heap: UnsafeCell::new([0; 1024 * 1024]),
    offset: UnsafeCell::new(0),
};

mod ipc;
mod service;

use core::panic::PanicInfo;
use sel4_sys::*;

use crate::ipc::{FsRequest, FsResponse, FS_ENDPOINT_SLOT};
use crate::service::FsService;

// Include entry point assembly
core::arch::global_asm!(include_str!("entry.S"));

/// Global filesystem service instance
static FS_SERVICE: spin::Lazy<FsService> = spin::Lazy::new(|| FsService::new());

/// Rust entry point
#[unsafe(export_name = "sel4_runtime_rust_entry")]
unsafe extern "C" fn rust_entry(bi_frame_vptr: usize) -> ! {
    // Initialize IPC buffer
    init_ipc_buffer(bi_frame_vptr);

    // Print startup message
    seL4_DebugPutString("[ext4-srv] Starting filesystem service...\n");

    // Main service loop
    service_loop();

    // Should never reach here
    loop {
        core::hint::spin_loop();
    }
}

/// Initialize IPC buffer
fn init_ipc_buffer(bi_frame_vptr: usize) {
    unsafe {
        let ipc_buf_ptr = (bi_frame_vptr - 4096) as *mut IpcBuffer;
        let ipc_buf = &mut *ipc_buf_ptr;
        ipc_buf.set_receive_slot(init_slots::CNODE, 0, 64);
        set_ipc_buffer(ipc_buf);
    }
}

/// Main service loop - listen for IPC requests
fn service_loop() {
    seL4_DebugPutString("[ext4-srv] Listening for requests...\n");

    loop {
        // Receive request
        let (tag, badge) = seL4_Recv(FS_ENDPOINT_SLOT);
        let msg = MessageInfo::from_word(tag);

        // Read request from IPC buffer
        let request = with_ipc_buffer(|ib| {
            // Read request type and parameters from message registers
            let req_type = ib.read_mr(0);
            let arg1 = ib.read_mr(1);
            let arg2 = ib.read_mr(2);
            let arg3 = ib.read_mr(3);
            let arg4 = ib.read_mr(4);

            match req_type {
                1 => FsRequest::Open {
                    path_addr: arg1,
                    path_len: arg2,
                    flags: arg3 as u32,
                },
                2 => FsRequest::Read {
                    fd: arg1,
                    buf_addr: arg2,
                    count: arg3,
                },
                3 => FsRequest::Write {
                    fd: arg1,
                    data_addr: arg2,
                    count: arg3,
                },
                4 => FsRequest::Close {
                    fd: arg1,
                },
                5 => FsRequest::Stat {
                    path_addr: arg1,
                    path_len: arg2,
                    stat_addr: arg3,
                },
                6 => FsRequest::Getdents64 {
                    fd: arg1,
                    buf_addr: arg2,
                    count: arg3,
                },
                7 => FsRequest::Mkdir {
                    path_addr: arg1,
                    path_len: arg2,
                },
                8 => FsRequest::Unlink {
                    path_addr: arg1,
                    path_len: arg2,
                },
                9 => FsRequest::Access {
                    path_addr: arg1,
                    path_len: arg2,
                    mode: arg3 as u32,
                },
                10 => FsRequest::FileSize {
                    fd: arg1,
                },
                _ => {
                    seL4_DebugPutString("[ext4-srv] Unknown request type: ");
                    // Can't print the number easily
                    seL4_DebugPutChar(b'\n');
                    FsRequest::Close { fd: 0 } // Dummy
                }
            }
        });

        // Handle request
        let response = FS_SERVICE.handle_request(&request);

        // Send response
        with_ipc_buffer(|ib| {
            match response {
                FsResponse::Ok(result) => {
                    ib.write_mr(0, 0); // Success
                    ib.write_mr(1, result);
                }
                FsResponse::Ok2(r1, r2) => {
                    ib.write_mr(0, 0); // Success
                    ib.write_mr(1, r1);
                    ib.write_mr(2, r2);
                }
                FsResponse::Err(errno) => {
                    ib.write_mr(0, 1); // Error
                    ib.write_mr(1, errno as usize);
                }
            }
        });

        // Reply
        let reply = MessageInfo::new(0, 0, 0);
        seL4_Reply(reply.word());
    }
}

/// Panic handler
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    seL4_DebugPutString("\n\n=== ext4-srv PANIC ===\n");
    seL4_DebugPutString("System halted.\n");
    loop {
        core::hint::spin_loop();
    }
}
