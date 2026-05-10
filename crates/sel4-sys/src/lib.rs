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
#[thread_local]
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
// ---------------------------------------------------------------------------

/// Syscall number for `seL4_Send`: Blocking send to an endpoint.
pub const SYS_SEND: isize = -1;
/// Syscall number for `seL4_NBSend`: Non-blocking send to an endpoint.
pub const SYS_NBSEND: isize = -2;
/// Syscall number for `seL4_Call`: Blocking call (send + wait for reply).
pub const SYS_CALL: isize = -3;
/// Syscall number for `seL4_Reply`: Send a reply to a pending call.
pub const SYS_REPLY: isize = -4;
/// Syscall number for `seL4_Recv`: Blocking receive from an endpoint.
pub const SYS_RECV: isize = -5;
/// Syscall number for `seL4_NBRecv`: Non-blocking receive from an endpoint.
pub const SYS_NBRECV: isize = -6;
/// Syscall number for `seL4_Yield`: Yield the current timeslice.
pub const SYS_YIELD: isize = -7;
/// Syscall number for `seL4_NBWait`: Non-blocking wait on a notification.
pub const SYS_NBWAIT: isize = -8;
/// Syscall number for `seL4_Poll`: Poll an endpoint or notification.
pub const SYS_POLL: isize = -9;
/// Syscall number for `seL4_DebugPutChar`: Output a character to kernel debug
/// serial.
pub const SYS_DEBUG_PUT_CHAR: isize = -10;
/// Syscall number for `seL4_DebugDumpScheduler`: Dump scheduler state (debug).
pub const SYS_DEBUG_DUMP_SCHEDULER: isize = -11;
/// Syscall number for `seL4_SetTLSBase`: Set the TLS base register (FS base on
/// x86_64) for the current thread.
pub const SYS_SET_TLS_BASE: isize = -12;

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
/// - `rdi` ← `arg0` (msg[0])
/// - `rsi` ← `arg1` (msg[1] / capability pointer)
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
/// - `rdi`  ← msg[0] (output)
/// - `rsi`  ← msg[1] (output)
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
    let mut out0: usize;
    let mut out1: usize;
    let mut out2: usize;
    let mut out3: usize;
    let mut out4: usize;
    let mut out5: usize;
    let mut out6: usize;
    let mut out7: usize;
    let mut status: usize;

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

            // Outputs
            lateout("rax") status,
            lateout("rdi") out0,
            lateout("rsi") out1,
            lateout("r10") out2,
            lateout("r8")  out3,
            lateout("r9")  out4,
            lateout("r12") out5,
            lateout("r13") out6,
            lateout("r15") out7,

            // Clobbers (syscall/sysret clobber these)
            lateout("rcx") _,
            lateout("r11") _,
            lateout("r14") _,

            options(nomem, nostack),
        );
    }

    (status, out0, out1, out2, out3, out4, out5, out6, out7)
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
            options(nomem, nostack),
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
            options(nomem, nostack),
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
            options(nomem, nostack),
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
            options(nomem, nostack),
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
            options(nomem, nostack),
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
            options(nomem, nostack),
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
            options(nomem, nostack),
        );
    }
    (status, out0, out1, out2, out3, out4, out5)
}
