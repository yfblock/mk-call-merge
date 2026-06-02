//! `sel4-sys` — Minimal seL4 system interface crate for x86_64.
//!
//! This crate provides:
//!
//! - **Syscall wrappers**: Inline assembly wrappers for all seL4 system calls
//!   (`Send`, `Recv`, `Call`, `Reply`, `Yield`, `NBRecv`, `DebugPutChar`, etc.).
//! - **Kernel ABI types**: `MessageInfo`, `UserContext`, `CNodeCapData`,
//!   `CapRights`, `Fault`, and their raw C-layout equivalents.
//! - **Capability types**: Marker types for all seL4 kernel objects (`Tcb`,
//!   `CNode`, `Endpoint`, `Notification`, `Untyped`, `VSpace`, `Frame`, `PageTable`,
//!   `PageDirectory`, `IrqControl`, `IrqHandler`, `AsidPool`, `AsidControl`,
//!   `PML4`, `PDPT`).
//! - **Capability operations**: `copy`, `mint`, `delete`, `revoke`, `retype`,
//!   `tcb_configure`, `tcb_write_registers`, `frame_map`, etc.
//! - **BootInfo**: Parsing the `seL4_BootInfo` structure provided by the kernel
//!   at boot time.
//! - **IPC buffer**: Thread-local IPC buffer management.
//!
//! # Architecture
//!
//! This crate only supports **x86_64**. All syscalls use the `syscall`
//! instruction with the standard seL4 calling convention:
//!
//! - Save `rsp` in `r14` before `syscall`, restore after (the kernel clobbers
//!   `rsp` when entering kernel mode).
//! - Syscall number passed in `rdx`.
//! - Message registers: `rdi`, `rsi`, `r10`, `r8`, `r9`, `r12`, `r13`, `r15`.
//!
//! # No FFI
//!
//! All code is pure Rust. There are no `extern "C"` blocks calling into C
//! libraries. The `#[repr(C)]` structs exist only for ABI compatibility with
//! the seL4 kernel binary interface — not for calling C functions.

#![no_std]
#![allow(internal_features)]
#![allow(non_snake_case)]
#![allow(dead_code)]
#![feature(thread_local)]

extern crate alloc;

// ---------------------------------------------------------------------------
// Public modules
// ---------------------------------------------------------------------------

pub mod bootinfo;
pub mod error;
pub mod ipc_buffer;
pub mod syscalls;
pub mod tests;
pub mod types;

// ---------------------------------------------------------------------------
// Re-exports for convenience
// ---------------------------------------------------------------------------

pub use bootinfo::{BootInfo, BootInfoRaw, UntypedDesc};
pub use error::{Error, Result};
pub use ipc_buffer::IpcBuffer;
pub use syscalls::*;
pub use types::*;

// ---------------------------------------------------------------------------
// Global TLS: thread-local IPC buffer pointer
// ---------------------------------------------------------------------------

/// Thread-local pointer to the current thread's IPC buffer.
///
/// Each seL4 thread must have a dedicated IPC buffer page. This variable holds
/// the mutable reference to it, used by all syscall wrappers.
static mut IPC_BUFFER_PTR: Option<&'static mut IpcBuffer> = None;

/// Set the IPC buffer for the current thread.
///
/// # Safety
///
/// This must only be called once per thread, before any syscalls that use the
/// IPC buffer. The provided buffer must be a valid, page-aligned IPC buffer
/// that will remain valid for the lifetime of the thread.
pub unsafe fn set_ipc_buffer(buf: &'static mut IpcBuffer) {
    unsafe {
        IPC_BUFFER_PTR = Some(buf);
    }
}

/// Get the raw address of the IPC buffer, or 0 if not initialized.
/// Returns a raw usize address — no reference is created, so no aliasing guarantees apply.
pub fn ipc_buffer_addr() -> usize {
    let opt_ptr: *mut Option<&'static mut IpcBuffer> =
        core::ptr::addr_of_mut!(IPC_BUFFER_PTR);
    unsafe {
        match &*opt_ptr {
            Some(buf) => &**buf as *const IpcBuffer as usize,
            None => 0,
        }
    }
}

/// Get a mutable reference to the current thread's IPC buffer.
///
/// # Panics
///
/// Panics if `set_ipc_buffer` has not been called for this thread.
pub fn with_ipc_buffer<F, R>(f: F) -> R
where
    F: FnOnce(&mut IpcBuffer) -> R,
{
    // SAFETY: We use raw pointer operations to avoid creating references
    // to mutable statics, which is denied in Rust 2024 edition.
    // Each thread has its own TLS IPC_BUFFER_PTR (it's #[thread_local]),
    // so there is no data race.
    let opt_ptr: *mut Option<&'static mut IpcBuffer> =
        core::ptr::addr_of_mut!(IPC_BUFFER_PTR);
    let buf_ref: &mut IpcBuffer = unsafe {
        match (*opt_ptr).as_deref_mut() {
            Some(buf) => buf,
            None => panic!("IPC buffer not initialized. Call set_ipc_buffer() first."),
        }
    };
    f(buf_ref)
}

// ---------------------------------------------------------------------------
// Constants: seL4 syscall numbers (x86_64)
//
// These match the kernel's `enum syscall` in `arch/api/syscall.h`.
// Note: x86_64 seL4 uses `syscall` instruction. The syscall number goes
// in RAX (not RDX). The capability goes in RDI, and message info in RSI.
// ---------------------------------------------------------------------------

/// Syscall number for `seL4_Call`: Blocking call (send + wait for reply).
pub const SYS_CALL: isize = -1;
/// Syscall number for `seL4_ReplyRecv`: Reply and receive (combined).
pub const SYS_REPLY_RECV: isize = -2;
/// Syscall number for `seL4_Send`: Blocking send to an endpoint.
pub const SYS_SEND: isize = -3;
/// Syscall number for `seL4_NBSend`: Non-blocking send to an endpoint.
pub const SYS_NBSEND: isize = -4;
/// Syscall number for `seL4_Recv`: Blocking receive from an endpoint.
pub const SYS_RECV: isize = -5;
/// Syscall number for `seL4_Reply`: Send a reply to a pending call.
pub const SYS_REPLY: isize = -6;
/// Syscall number for `seL4_Yield`: Yield the current timeslice.
pub const SYS_YIELD: isize = -7;
/// Syscall number for `seL4_NBRecv`: Non-blocking receive from an endpoint.
pub const SYS_NBRECV: isize = -8;
/// Syscall number for `seL4_DebugPutChar`: Output a character to kernel debug
/// serial.
pub const SYS_DEBUG_PUT_CHAR: isize = -9;
/// Syscall number for `seL4_DebugDumpScheduler`: Dump scheduler state (debug).
pub const SYS_DEBUG_DUMP_SCHEDULER: isize = -10;
/// Syscall number for `seL4_DebugHalt`: Halt the kernel (shutdown).
pub const SYS_DEBUG_HALT: isize = -11;
/// Syscall number for `seL4_Signal`: Signal a notification object.
pub const SYS_SIGNAL: isize = -12;
/// Syscall number for `seL4_SetTLSBase`: Set the TLS base register (FS base on
/// x86_64) for the current thread.
pub const SYS_SET_TLS_BASE: isize = -29;

// ---------------------------------------------------------------------------
// Core syscall ABI: the actual `syscall` instruction
// ---------------------------------------------------------------------------

/// Execute a raw seL4 system call with up to 8 word arguments.
///
/// This is the fundamental building block for all seL4 syscalls. On x86_64,
/// the calling convention is:
///
/// - `r14` ← `rsp` (save user stack pointer, clobbered by kernel)
/// - `rdx` ← syscall number
/// - `rdi` ← `arg0` (capability dest/src)
/// - `rsi` ← `arg1` (message info / tag)
/// - `r10` ← `arg2` (msg[2])
/// - `r8`  ← `arg3` (msg[3])
/// - `r9`  ← `arg4` (msg[4])
/// - `r12` ← `arg5` (msg[5])
/// - `r13` ← `arg6` (msg[6])
/// - `r15` ← `arg7` (msg[7])
///
/// After `syscall`:
///
/// - `rax`  ← status / result
/// - `rdi`  ← cap register (output)
/// - `rsi`  ← badge (output)
/// - `r10`  ← msg[2] (output)
/// - `r8`   ← msg[3] (output)
/// - `r9`   ← msg[4] (output)
/// - `r12`  ← msg[5] (output)
/// - `r13`  ← msg[6] (output)
/// - `r15`  ← msg[7] (output)
/// - `r14` ← `rsp` (restore user stack pointer)
/// - `rcx`, `r11` are clobbered (syscall/sysret convention)
///
/// # Safety
///
/// This is inherently unsafe; it executes a raw system call instruction.
/// The caller must ensure that all capability pointers are valid and that
/// the operation makes sense for the current state.
#[inline(always)]
unsafe fn seL4_syscall(
    sys: isize,
    arg0: usize,
    arg1: usize,
    arg2: usize,
    arg3: usize,
    arg4: usize,
    arg5: usize,
    arg6: usize,
    arg7: usize,
) -> (usize, usize, usize, usize, usize, usize, usize, usize, usize) {
    let mut tag: usize;
    let mut badge: usize;
    let mut mr0: usize;
    let mut mr1: usize;
    let mut mr2: usize;
    let mut mr3: usize;
    let mut _r12: usize;
    let mut _r13: usize;

    unsafe {
        core::arch::asm!(
            // Save user stack pointer in r14 before entering kernel
            "mov r14, rsp",
            // Enter kernel
            "syscall",
            // Restore user stack pointer after kernel returns
            "mov rsp, r14",

            // Inputs
            in("rdx") sys,
            in("rdi") arg0,
            in("rsi") arg1,
            in("r10") arg2,
            in("r8")  arg3,
            in("r9")  arg4,
            in("r12") arg5,
            in("r13") arg6,
            in("r15") arg7,

            // Outputs — kernel return register mapping (from registerset.h):
            // RDI = badge (badgeRegister = 0)
            // RSI = msgInfo (msgInfoRegister = 1)
            // R10, R8, R9, R15 = message registers MR[0..3]
            lateout("rax") _,
            lateout("rdi") badge,
            lateout("rsi") tag,
            lateout("r10") mr0,
            lateout("r8")  mr1,
            lateout("r9")  mr2,
            lateout("r12") _r12,
            lateout("r13") _r13,
            lateout("r15") mr3,

            // Clobbers (syscall/sysret clobber these)
            lateout("rcx") _,
            lateout("r11") _,
            lateout("r14") _,

            options(nostack),
        );
    }

    (0, tag, badge, mr0, mr1, mr2, mr3, 0, 0)
}

/// Write MR4-MR9 to the IPC buffer, then execute a seL4 syscall.
///
/// The kernel's `getSyscallArg(i, buffer)` reads `buffer[i+1]` where
/// `buffer` points to `&(ipcBuffer->tag)` (the start of the struct).
/// Since `msg[0]` is at word offset 1 from the struct start:
///   `buffer[i+1]` = `msg[i]` (byte offset `(i+1)*8` from struct start).
///
/// `buf_addr` = `ipc_buffer_addr() + 8` = address of `msg[0]`.
/// So to write `msg[i]`, use `buf_addr + i*8`.
#[inline(always)]
pub(crate) unsafe fn seL4_syscall_with_buf(
    sys: isize,
    arg0: usize, arg1: usize, arg2: usize, arg3: usize,
    arg4: usize, arg5: usize, arg6: usize, arg7: usize,
    buf_addr: usize,
    mr4: usize, mr5: usize, mr6: usize, mr7: usize, mr8: usize, mr9: usize,
) -> (usize, usize, usize, usize, usize, usize, usize, usize, usize) {
    unsafe {
        // buf_addr = &msg[0]. Write msg[i] = buf_addr + i*8.
        // getSyscallArg(4) reads msg[4], getSyscallArg(5) reads msg[5], etc.
        core::ptr::write_volatile((buf_addr + 32) as *mut usize, mr4);  // msg[4]
        core::ptr::write_volatile((buf_addr + 40) as *mut usize, mr5);  // msg[5]
        core::ptr::write_volatile((buf_addr + 48) as *mut usize, mr6);  // msg[6]
        core::ptr::write_volatile((buf_addr + 56) as *mut usize, mr7);  // msg[7]
        core::ptr::write_volatile((buf_addr + 64) as *mut usize, mr8);  // msg[8]
        core::ptr::write_volatile((buf_addr + 72) as *mut usize, mr9);  // msg[9]
        // Hardware memory fence + compiler barrier.
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        seL4_syscall(sys, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7)
    }
}

/// Write extra caps and MR4-MR9 to the IPC buffer, then execute a seL4 syscall.
///
/// Extra caps are written to `caps_or_badges[]` at byte offsets 976, 984, 992
/// from the IPC buffer start (i.e., `buf_addr + 976 + i*8`).
#[inline(always)]
pub(crate) unsafe fn seL4_syscall_with_caps(
    sys: isize,
    arg0: usize, arg1: usize, arg2: usize, arg3: usize,
    arg4: usize, arg5: usize, arg6: usize, arg7: usize,
    buf_addr: usize,
    mr4: usize, mr5: usize, mr6: usize, mr7: usize, mr8: usize, mr9: usize,
    num_caps: usize, cap0: usize, cap1: usize, cap2: usize,
) -> (usize, usize, usize, usize, usize, usize, usize, usize, usize) {
    unsafe {
        // Write extra caps to IPC buffer caps_or_badges[0..2].
        // caps_or_badges is at byte offset 976 from buffer start (&tag).
        if num_caps >= 1 {
            core::ptr::write_volatile((buf_addr + 976) as *mut usize, cap0);
        }
        if num_caps >= 2 {
            core::ptr::write_volatile((buf_addr + 984) as *mut usize, cap1);
        }
        if num_caps >= 3 {
            core::ptr::write_volatile((buf_addr + 992) as *mut usize, cap2);
        }
        // Write MR4-MR9 to IPC buffer msg[] area.
        // msg[] starts at byte offset 8 from buffer start.
        let msg = buf_addr + 8;
        core::ptr::write_volatile((msg + 32) as *mut usize, mr4);
        core::ptr::write_volatile((msg + 40) as *mut usize, mr5);
        core::ptr::write_volatile((msg + 48) as *mut usize, mr6);
        core::ptr::write_volatile((msg + 56) as *mut usize, mr7);
        core::ptr::write_volatile((msg + 64) as *mut usize, mr8);
        core::ptr::write_volatile((msg + 72) as *mut usize, mr9);
        // Hardware memory fence + compiler barrier.
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

        seL4_syscall(sys, arg0, arg1, arg2, arg3, arg4, arg5, arg6, arg7)
    }
}

/// Execute a raw seL4 system call with no arguments (only the syscall number).
///
/// Used for simple operations like `Yield`, `DebugPutChar`, etc.
#[inline(always)]
pub unsafe fn sys_null(sys: isize) {
    unsafe {
        core::arch::asm!(
            "mov r14, rsp",
            "syscall",
            "mov rsp, r14",
            in("rdx") sys,
            lateout("rcx") _,
            lateout("r11") _,
            lateout("r14") _,
            options(nostack),
        );
    }
}

/// Execute a seL4 system call with exactly 1 word argument.
#[inline(always)]
pub unsafe fn sys_send1(sys: isize, a0: usize) -> (usize, usize) {
    let mut status: usize;
    let mut out0: usize;
    unsafe {
        core::arch::asm!(
            "mov r14, rsp",
            "syscall",
            "mov rsp, r14",
            in("rdx") sys,
            in("rdi") a0,
            lateout("rax") status,
            lateout("rdi") out0,
            lateout("rcx") _,
            lateout("r11") _,
            lateout("r14") _,
            options(nostack),
        );
    }
    (status, out0)
}

/// Execute a seL4 system call with exactly 2 word arguments.
#[inline(always)]
pub unsafe fn sys_send2(sys: isize, a0: usize, a1: usize) -> (usize, usize, usize) {
    let mut status: usize;
    let mut out0: usize;
    let mut out1: usize;
    unsafe {
        core::arch::asm!(
            "mov r14, rsp",
            "syscall",
            "mov rsp, r14",
            in("rdx") sys,
            in("rdi") a0,
            in("rsi") a1,
            lateout("rax") status,
            lateout("rdi") out0,
            lateout("rsi") out1,
            lateout("rcx") _,
            lateout("r11") _,
            lateout("r14") _,
            options(nostack),
        );
    }
    (status, out0, out1)
}

/// Execute a seL4 system call with exactly 3 word arguments.
#[inline(always)]
pub unsafe fn sys_send3(
    sys: isize,
    a0: usize,
    a1: usize,
    a2: usize,
) -> (usize, usize, usize, usize) {
    let mut status: usize;
    let mut out0: usize;
    let mut out1: usize;
    let mut out2: usize;
    unsafe {
        core::arch::asm!(
            "mov r14, rsp",
            "syscall",
            "mov rsp, r14",
            in("rdx") sys,
            in("rdi") a0,
            in("rsi") a1,
            in("r10") a2,
            lateout("rax") status,
            lateout("rdi") out0,
            lateout("rsi") out1,
            lateout("r10") out2,
            lateout("rcx") _,
            lateout("r11") _,
            lateout("r14") _,
            options(nostack),
        );
    }
    (status, out0, out1, out2)
}

/// Execute a seL4 system call with exactly 4 word arguments.
#[inline(always)]
pub unsafe fn sys_send4(
    sys: isize,
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
) -> (usize, usize, usize, usize, usize) {
    let mut status: usize;
    let mut out0: usize;
    let mut out1: usize;
    let mut out2: usize;
    let mut out3: usize;
    unsafe {
        core::arch::asm!(
            "mov r14, rsp",
            "syscall",
            "mov rsp, r14",
            in("rdx") sys,
            in("rdi") a0,
            in("rsi") a1,
            in("r10") a2,
            in("r8")  a3,
            lateout("rax") status,
            lateout("rdi") out0,
            lateout("rsi") out1,
            lateout("r10") out2,
            lateout("r8")  out3,
            lateout("rcx") _,
            lateout("r11") _,
            lateout("r14") _,
            options(nostack),
        );
    }
    (status, out0, out1, out2, out3)
}

/// Execute a seL4 system call with exactly 5 word arguments.
#[inline(always)]
pub unsafe fn sys_send5(
    sys: isize,
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
) -> (usize, usize, usize, usize, usize, usize) {
    let mut status: usize;
    let mut out0: usize;
    let mut out1: usize;
    let mut out2: usize;
    let mut out3: usize;
    let mut out4: usize;
    unsafe {
        core::arch::asm!(
            "mov r14, rsp",
            "syscall",
            "mov rsp, r14",
            in("rdx") sys,
            in("rdi") a0,
            in("rsi") a1,
            in("r10") a2,
            in("r8")  a3,
            in("r9")  a4,
            lateout("rax") status,
            lateout("rdi") out0,
            lateout("rsi") out1,
            lateout("r10") out2,
            lateout("r8")  out3,
            lateout("r9")  out4,
            lateout("rcx") _,
            lateout("r11") _,
            lateout("r14") _,
            options(nostack),
        );
    }
    (status, out0, out1, out2, out3, out4)
}

/// Execute a seL4 system call with exactly 6 word arguments.
#[inline(always)]
pub unsafe fn sys_send6(
    sys: isize,
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
) -> (usize, usize, usize, usize, usize, usize, usize) {
    let mut status: usize;
    let mut out0: usize;
    let mut out1: usize;
    let mut out2: usize;
    let mut out3: usize;
    let mut out4: usize;
    let mut out5: usize;
    unsafe {
        core::arch::asm!(
            "mov r14, rsp",
            "syscall",
            "mov rsp, r14",
            in("rdx") sys,
            in("rdi") a0,
            in("rsi") a1,
            in("r10") a2,
            in("r8")  a3,
            in("r9")  a4,
            in("r12") a5,
            lateout("rax") status,
            lateout("rdi") out0,
            lateout("rsi") out1,
            lateout("r10") out2,
            lateout("r8")  out3,
            lateout("r9")  out4,
            lateout("r12") out5,
            lateout("rcx") _,
            lateout("r11") _,
            lateout("r14") _,
            options(nostack),
        );
    }
    (status, out0, out1, out2, out3, out4, out5)
}
