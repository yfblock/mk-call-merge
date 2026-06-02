#![no_std]
#![no_main]

extern crate alloc;

use alloc::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};
use blk_task::{BlockIface, RamdiskBlkImpl, BLOCK_SIZE};
use sel4_sys::*;

// 简单的 bump allocator（128KB 堆）
const HEAP_SIZE: usize = 128 * 1024;
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

// IPC 消息类型
const MSG_READ: usize = 2;
const MSG_WRITE: usize = 3;
const MSG_CAPACITY: usize = 4;

core::arch::global_asm!(include_str!("entry.S"));

#[unsafe(export_name = "rust_entry")]
extern "C" fn rust_entry(ipc_buf_addr: usize) -> ! {
    // 设置 IPC buffer（由 root-task 映射到 ipc_buf_addr）
    if ipc_buf_addr != 0 {
        let buf = unsafe { &mut *(ipc_buf_addr as *mut IpcBuffer) };
        unsafe { set_ipc_buffer(buf) };
    }

    let blk = RamdiskBlkImpl::new();

    seL4_DebugPutString("[blk-task] started, capacity: ");
    print_u64(blk.capacity());
    seL4_DebugPutString(" bytes\n");

    // 监听 RDI 中传递的 endpoint slot（由 root-task 通过寄存器传入）
    // 这里使用约定的 slot 20
    let ep_slot: usize = 20;

    loop {
        let (tag, _badge) = seL4_Recv(ep_slot);
        let msg = MessageInfo::from_word(tag);
        let msg_type = msg.label() as usize;

        match msg_type {
            MSG_READ => {
                let (block_id, block_num, buf_addr) = with_ipc_buffer(|ib| {
                    (ib.read_mr(0), ib.read_mr(1), ib.read_mr(2))
                });

                for i in 0..block_num {
                    let offset = buf_addr + i * BLOCK_SIZE;
                    let buf = unsafe {
                        core::slice::from_raw_parts_mut(offset as *mut u8, BLOCK_SIZE)
                    };
                    blk.read_block(block_id + i, buf);
                }

                let reply = MessageInfo::new(0, 1, 0);
                with_ipc_buffer(|ib| {
                    ib.write_mr(0, 0); // success
                    seL4_Reply(reply.word());
                });
            }
            MSG_WRITE => {
                let (block_id, block_num, buf_addr) = with_ipc_buffer(|ib| {
                    (ib.read_mr(0), ib.read_mr(1), ib.read_mr(2))
                });

                for i in 0..block_num {
                    let offset = buf_addr + i * BLOCK_SIZE;
                    let buf = unsafe {
                        core::slice::from_raw_parts(offset as *const u8, BLOCK_SIZE)
                    };
                    blk.write_block(block_id + i, buf);
                }

                let reply = MessageInfo::new(0, 1, 0);
                with_ipc_buffer(|ib| {
                    ib.write_mr(0, 0); // success
                    seL4_Reply(reply.word());
                });
            }
            MSG_CAPACITY => {
                let reply = MessageInfo::new(0, 1, 0);
                with_ipc_buffer(|ib| {
                    ib.write_mr(0, blk.capacity() as usize);
                    seL4_Reply(reply.word());
                });
            }
            _ => {
                seL4_DebugPutString("[blk-task] unknown message\n");
                let reply = MessageInfo::new(0, 1, 0);
                with_ipc_buffer(|ib| {
                    ib.write_mr(0, usize::MAX); // error
                    seL4_Reply(reply.word());
                });
            }
        }
    }
}

fn print_u64(val: u64) {
    if val == 0 {
        seL4_DebugPutChar(b'0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 0;
    let mut v = val;
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i += 1;
    }
    for j in (0..i).rev() {
        seL4_DebugPutChar(buf[j]);
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    seL4_DebugPutString("[blk-task] PANIC\n");
    loop {
        core::hint::spin_loop();
    }
}
