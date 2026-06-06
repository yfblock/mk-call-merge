//! ext4 filesystem service for seL4
//!
//! This service provides file operations via IPC for the Linux Compatible Layer.
//!
//! ## IPC Protocol (mirrored in `lcl/src/fs/ipc_client.rs`)
//!
//! **Request**:  MR0=req_type, MR1..MR4=args, MR5+=path bytes (for path ops)
//! **Response**: MR0=status(0=ok,1=err), MR1=result, MR2+=data (for data ops)
//!              The reply `MessageInfo.length` is set so the kernel copies all
//!              populated MRs.

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

/// MR index where path payload starts in requests
const PATH_MR_START: usize = 5;

/// MR index where data payload starts in responses
const DATA_MR_START: usize = 2; // MR0=status, MR1=result, MR2+=data

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

/// Decode path bytes from IPC MRs starting at `start_mr`.
fn decode_path_from_mrs(ib: &IpcBuffer, start_mr: usize, path_len: usize) -> alloc::string::String {
    use alloc::vec::Vec;
    let mut bytes = Vec::with_capacity(path_len);
    let num_mrs = (path_len + 7) / 8;
    for i in 0..num_mrs {
        let word = ib.read_mr(start_mr + i);
        for j in 0..8 {
            let idx = i * 8 + j;
            if idx < path_len {
                bytes.push(((word >> (j * 8)) & 0xFF) as u8);
            }
        }
    }
    alloc::string::String::from_utf8_lossy(&bytes).into_owned()
}

/// Encode data bytes into IPC MRs starting at `start_mr`.
/// Returns the number of MRs used.
fn encode_data_to_mrs(ib: &mut IpcBuffer, data: &[u8], start_mr: usize) -> usize {
    let num_mrs = (data.len() + 7) / 8;
    for i in 0..num_mrs {
        let mut word: usize = 0;
        for j in 0..8 {
            let idx = i * 8 + j;
            if idx < data.len() {
                word |= (data[idx] as usize) << (j * 8);
            }
        }
        ib.write_mr(start_mr + i, word);
    }
    num_mrs
}

/// Main service loop - listen for IPC requests
fn service_loop() {
    seL4_DebugPutString("[ext4-srv] Listening for requests...\n");

    loop {
        // Receive request
        let (tag, _badge) = seL4_Recv(FS_ENDPOINT_SLOT);
        let _msg = MessageInfo::from_word(tag);

        // Decode request and handle it
        let reply_mrs = with_ipc_buffer(|ib| {
            let req_type = ib.read_mr(0);
            let arg1 = ib.read_mr(1);
            let arg2 = ib.read_mr(2);
            let arg3 = ib.read_mr(3);
            let _arg4 = ib.read_mr(4);

            match req_type {
                // ── path-bearing requests ────────────────────────
                1 => {
                    // Open: arg1=unused, arg2=path_len, arg3=flags
                    let path = decode_path_from_mrs(ib, PATH_MR_START, arg2);
                    let response = FS_SERVICE.handle_open_with_path(&path, arg3 as u32);
                    write_response(ib, &response)
                }
                5 => {
                    // Stat: arg1=unused, arg2=path_len
                    let path = decode_path_from_mrs(ib, PATH_MR_START, arg2);
                    let response = FS_SERVICE.handle_stat_with_path(&path);
                    write_response(ib, &response)
                }
                7 => {
                    // Mkdir
                    let path = decode_path_from_mrs(ib, PATH_MR_START, arg2);
                    let response = FS_SERVICE.handle_mkdir_with_path(&path);
                    write_response(ib, &response)
                }
                8 => {
                    // Unlink
                    let path = decode_path_from_mrs(ib, PATH_MR_START, arg2);
                    let response = FS_SERVICE.handle_unlink_with_path(&path);
                    write_response(ib, &response)
                }
                9 => {
                    // Access
                    let path = decode_path_from_mrs(ib, PATH_MR_START, arg2);
                    let response = FS_SERVICE.handle_access_with_path(&path);
                    write_response(ib, &response)
                }

                // ── fd-based requests ───────────────────────────
                2 => {
                    // Read: arg1=fd, arg2=unused, arg3=count
                    let request = FsRequest::Read {
                        fd: arg1,
                        buf_addr: 0,
                        count: arg3,
                    };
                    let response = FS_SERVICE.handle_request(&request);
                    write_response_maybe_data(ib, &response)
                }
                3 => {
                    // Write: arg1=fd, arg2=unused, arg3=count, MR2+=data
                    let data = decode_path_from_mrs(ib, DATA_MR_START, arg3);
                    let response = FS_SERVICE.handle_write_with_data(arg1, data.as_bytes());
                    write_response(ib, &response)
                }
                4 => {
                    let request = FsRequest::Close { fd: arg1 };
                    let response = FS_SERVICE.handle_request(&request);
                    write_response(ib, &response)
                }
                6 => {
                    // Getdents64: arg1=fd, arg2=unused, arg3=count
                    let request = FsRequest::Getdents64 {
                        fd: arg1,
                        buf_addr: 0,
                        count: arg3,
                    };
                    let response = FS_SERVICE.handle_request(&request);
                    write_response_maybe_data(ib, &response)
                }
                10 => {
                    let request = FsRequest::FileSize { fd: arg1 };
                    let response = FS_SERVICE.handle_request(&request);
                    write_response(ib, &response)
                }
                11 => {
                    // Lseek: arg1=fd, arg2=offset, arg3=whence
                    let response = FS_SERVICE.handle_lseek(arg1, arg2 as isize, arg3 as i32);
                    write_response(ib, &response)
                }
                _ => {
                    seL4_DebugPutString("[ext4-srv] Unknown request type\n");
                    ib.write_mr(0, RESP_ERR);
                    ib.write_mr(1, 38); // ENOSYS
                    2
                }
            }
        });

        // Reply with the correct MR count so the kernel copies them all
        let reply_word = reply_mrs & 0x7F;
        seL4_Reply(reply_word);
    }
}

const RESP_OK: usize = 0;
const RESP_ERR: usize = 1;

/// Write a simple (no-data) response into the IPC buffer.
/// Returns the number of MRs written.
fn write_response(ib: &mut IpcBuffer, resp: &FsResponse) -> usize {
    match resp {
        FsResponse::Ok(val) => {
            ib.write_mr(0, RESP_OK);
            ib.write_mr(1, *val);
            2
        }
        FsResponse::Ok2(a, b) => {
            ib.write_mr(0, RESP_OK);
            ib.write_mr(1, *a);
            ib.write_mr(2, *b);
            3
        }
        FsResponse::Err(errno) => {
            ib.write_mr(0, RESP_ERR);
            ib.write_mr(1, *errno as usize);
            2
        }
        // For data-carrying responses used through write_response (shouldn't happen),
        // fall back to encoding as data
        FsResponse::OkWithData(data) => {
            ib.write_mr(0, RESP_OK);
            ib.write_mr(1, data.len());
            let data_mrs = encode_data_to_mrs(ib, data, DATA_MR_START);
            DATA_MR_START + data_mrs
        }
        FsResponse::OkWithData2(data, extra) => {
            ib.write_mr(0, RESP_OK);
            ib.write_mr(1, data.len());
            ib.write_mr(2, *extra);
            let data_mrs = encode_data_to_mrs(ib, data, 3);
            3 + data_mrs
        }
    }
}

/// Write a response that may carry data payload in MR2+.
/// For `OkWithData(data)` the data bytes are encoded in MRs starting at
/// `DATA_MR_START`.  Returns the total MR count.
fn write_response_maybe_data(ib: &mut IpcBuffer, resp: &FsResponse) -> usize {
    match resp {
        FsResponse::Ok(val) => {
            ib.write_mr(0, RESP_OK);
            ib.write_mr(1, *val);
            2
        }
        FsResponse::Ok2(a, b) => {
            ib.write_mr(0, RESP_OK);
            ib.write_mr(1, *a);
            ib.write_mr(2, *b);
            3
        }
        FsResponse::OkWithData(data) => {
            ib.write_mr(0, RESP_OK);
            ib.write_mr(1, data.len());
            let data_mrs = encode_data_to_mrs(ib, data, DATA_MR_START);
            DATA_MR_START + data_mrs
        }
        FsResponse::OkWithData2(data, extra) => {
            ib.write_mr(0, RESP_OK);
            ib.write_mr(1, data.len());
            ib.write_mr(2, *extra);
            let data_mrs = encode_data_to_mrs(ib, data, 3); // MR3+=data
            3 + data_mrs
        }
        FsResponse::Err(errno) => {
            ib.write_mr(0, RESP_ERR);
            ib.write_mr(1, *errno as usize);
            2
        }
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
