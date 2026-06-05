//! Task initialization - stack setup and TCB configuration
//!
//! Ported from kernel-thread with x86_64 adaptations.

use alloc::vec::Vec;
use common::config::PAGE_SIZE;
use sel4_sys::*;

use super::Sel4Task;
use crate::consts::task::*;

/// x86_64 Linux auxiliary vector types
pub const AT_NULL: usize = 0;
pub const AT_PHDR: usize = 3;
pub const AT_PHENT: usize = 4;
pub const AT_PHNUM: usize = 5;
pub const AT_PAGESZ: usize = 6;
pub const AT_ENTRY: usize = 9;
pub const AT_UID: usize = 11;
pub const AT_GID: usize = 12;
pub const AT_SECURE: usize = 23;
pub const AT_RANDOM: usize = 25;
pub const AT_SYSINFO_EHDR: usize = 33;

/// Initialize the task's stack with Linux ABI layout
///
/// Stack layout (high to low):
/// - Argument strings
/// - Environment strings
/// - Auxiliary vectors (AT_*)
/// - Environment pointers
/// - Argument pointers
/// - argc
pub fn init_stack(
    task: &Sel4Task,
    entry: usize,
    args: &[&str],
) -> usize {
    let mut sp = DEF_STACK_TOP;

    // Push argument strings
    let mut arg_ptrs = Vec::new();
    for arg in args.iter().rev() {
        sp -= arg.len() + 1;
        sp &= !7; // 8-byte align
        // Write arg string (in real impl, would write to mapped page)
        arg_ptrs.push(sp);
    }
    arg_ptrs.reverse();

    // Push null terminator
    sp -= 8;
    sp &= !7;

    // Push environment pointers (empty)
    sp -= 8;
    sp &= !7;

    // Push auxiliary vectors
    let auxv = [
        (AT_PAGESZ, PAGE_SIZE),
        (AT_ENTRY, entry),
        (AT_UID, 0),
        (AT_GID, 0),
        (AT_NULL, 0),
    ];
    for (key, val) in auxv.iter().rev() {
        sp -= 8;
        sp &= !7;
        // Write key
        sp -= 8;
        sp &= !7;
        // Write value
    }

    // Push null terminator for envp
    sp -= 8;
    sp &= !7;

    // Push environment pointers (empty)
    sp -= 8;
    sp &= !7;

    // Push null terminator for argv
    sp -= 8;
    sp &= !7;

    // Push argument pointers
    for _ in args.iter().rev() {
        sp -= 8;
        sp &= !7;
    }

    // Push argc
    sp -= 8;
    sp &= !7;

    sp
}

/// Configure the TCB with CSpace, VSpace, and scheduling params
pub fn configure_tcb(task: &Sel4Task) -> bool {
    // Configure TCB: fault_ep=0, cspace_root=task.cnode, vspace=task.vspace
    let err = seL4_TCB_Configure(
        task.tcb,
        0,                    // fault endpoint
        task.cnode,           // cspace root
        0,                    // cspace root data
        task.vspace,          // vspace
        0,                    // ipc buffer addr
        0,                    // ipc buffer frame
    );
    if err != 0 {
        sel4_sys::seL4_DebugPutString("[lcl] TCB_Configure failed\n");
        return false;
    }

    // Set scheduling params: authority=tcb, mcp=255, priority=255
    let err = seL4_TCB_SetSchedParams(
        task.tcb,
        task.tcb,  // authority
        255,       // mcp
        255,       // priority
    );
    if err != 0 {
        sel4_sys::seL4_DebugPutString("[lcl] SetSchedParams failed\n");
        return false;
    }

    true
}

/// Write initial register context and start the task
pub fn start_task(task: &Sel4Task, entry: usize, sp: usize) -> bool {
    // seL4_UserContext for x86_64:
    // 0: RAX, 1: RBX, 2: RCX, 3: RDX, 4: RSI, 5: RDI
    // 6: RBP, 7: RSP, 8: R8, 9: R9, 10: R10, 11: R11
    // 12: R12, 13: R13, 14: R14, 15: RIP, 16: RFLAGS
    let mut regs = [0usize; 20];
    regs[7] = sp;        // RSP
    regs[15] = entry;    // RIP
    regs[16] = 0x202;    // RFLAGS (IF set)

    let err = seL4_TCB_WriteRegisters(
        task.tcb,
        true,   // resume
        0,      // arch_flags
        20,     // count
        &regs,
    );
    if err != 0 {
        sel4_sys::seL4_DebugPutString("[lcl] WriteRegisters failed\n");
        return false;
    }

    true
}
