//! Exception/fault handling for x86_64 - ported from kernel-thread
//!
//! Handles faults from child tasks. On x86_64, syscalls are intercepted by
//! patching the `syscall` instruction (0x0f05) to `0xdeadbeef` in the ELF,
//! causing a user exception that we handle as a syscall.
//!
//! Key difference from aarch64: x86_64 uses `syscall` instruction (0x0f 0x05)
//! instead of `svc #0` (0xd4000001).

use common::config::PAGE_SIZE;
use sel4_sys::*;
use spin::Lazy;

use crate::child_test::TASK_MAP;
use crate::syscall::handle_syscall;
use crate::task::Sel4Task;

/// x86_64 syscall instruction bytes: 0x0f 0x05
/// When patched, becomes 0xdeadbeef (4 bytes)
const TRAP_INST: u32 = 0xdeadbeef;

/// seL4 UserContext register indices for x86_64
const REG_RAX: usize = 0;
const REG_RBX: usize = 1;
const REG_RCX: usize = 2;
const REG_RDX: usize = 3;
const REG_RSI: usize = 4;
const REG_RDI: usize = 5;
const REG_RBP: usize = 6;
const REG_RSP: usize = 7;
const REG_R8: usize = 8;
const REG_R9: usize = 9;
const REG_R10: usize = 10;
const REG_R11: usize = 11;
const REG_RIP: usize = 15;
const REG_RFLAGS: usize = 16;

/// Handle user exception (syscall or real fault)
///
/// On x86_64, when a patched `syscall` instruction (0xdeadbeef) is executed,
/// seL4 generates a UserException fault. We detect this by reading the
/// faulting instruction and checking if it's our trap value.
pub async fn handle_user_exception(tid: usize, fault_ip: usize, _reason: usize) {
    let task = {
        let map = TASK_MAP.lock();
        map.get(&tid).cloned()
    };
    let task = match task {
        Some(t) => t,
        None => {
            seL4_DebugPutString("[lcl] Unknown task exception\n");
            return;
        }
    };

    // Read the faulting instruction
    let ins = task.read_bytes_u32(fault_ip);

    if ins == Some(TRAP_INST) {
        // This is a syscall - read registers from IPC buffer
        // seL4 passes the register state in the fault message
        let mut regs = [0usize; 20];

        // Read registers from IPC buffer (seL4 puts them in MRs)
        with_ipc_buffer(|ib| {
            for i in 0..20 {
                regs[i] = ib.read_mr(i);
            }
        });

        let result = handle_syscall(&task, &mut regs).await;
        let ret_v = match result {
            Ok(v) => v,
            Err(e) => -(e as isize) as usize,
        };

        // Write return value to RAX
        regs[REG_RAX] = ret_v;
        // Advance PC past the trap instruction (4 bytes for 0xdeadbeef)
        regs[REG_RIP] += 4;

        if task.exit.lock().is_some() {
            return;
        }

        // Write back registers and resume
        let _ = seL4_TCB_WriteRegisters(task.tcb, true, 0, 20, &regs);
    } else {
        seL4_DebugPutString("[lcl] Unhandled user exception at ");
        let mut ip = fault_ip;
        for i in (0..16).rev() {
            let nibble = (ip >> (i * 4)) & 0xf;
            let c = if nibble < 10 { b'0' + nibble as u8 } else { b'a' + (nibble - 10) as u8 };
            seL4_DebugPutChar(c);
        }
        seL4_DebugPutChar(b'\n');
    }
}

/// Handle VM fault (demand paging)
///
/// On x86_64, when a task accesses unmapped memory, seL4 generates a VmFault.
/// We handle this by mapping a blank page at the faulting address.
pub fn handle_vmfault(tid: usize, addr: usize) {
    let vaddr = addr & !(PAGE_SIZE - 1);
    let map = TASK_MAP.lock();
    if let Some(task) = map.get(&tid) {
        // Map a blank page at the faulting address
        task.map_blank_page_simple(vaddr);
        seL4_DebugPutString("[lcl] VM fault: mapped page at ");
        let mut a = vaddr;
        for i in (0..16).rev() {
            let nibble = (a >> (i * 4)) & 0xf;
            let c = if nibble < 10 { b'0' + nibble as u8 } else { b'a' + (nibble - 10) as u8 };
            seL4_DebugPutChar(c);
        }
        seL4_DebugPutChar(b'\n');

        // Resume the task
        let regs = [0usize; 20];
        let _ = seL4_TCB_WriteRegisters(task.tcb, true, 0, 20, &regs);
    }
}

/// Handle UnknownSyscall fault
///
/// On x86_64, when a task executes `syscall` instruction, seL4 generates
/// an UnknownSyscall fault (not UserException). We handle this directly.
pub async fn handle_unknown_syscall(tid: usize, syscall_no: usize) {
    let task = {
        let map = TASK_MAP.lock();
        map.get(&tid).cloned()
    };
    let task = match task {
        Some(t) => t,
        None => return,
    };

    // Read registers from the fault message
    let mut regs = [0usize; 20];
    // The syscall number is in the fault message
    regs[0] = syscall_no; // RAX

    let result = handle_syscall(&task, &mut regs).await;
    let ret_v = match result {
        Ok(v) => v,
        Err(e) => -(e as isize) as usize,
    };

    regs[REG_RAX] = ret_v;
    let _ = seL4_TCB_WriteRegisters(task.tcb, true, 0, 20, &regs);
}

/// Dispatch fault based on type
pub async fn handle_fault(tid: usize, message: MessageInfo) {
    let label = message.label();

    match label {
        // VmFault
        1 => {
            let addr = with_ipc_buffer(|ib| ib.read_mr(0));
            handle_vmfault(tid, addr);
        }
        // UserException
        2 => {
            let (fault_ip, reason) = with_ipc_buffer(|ib| {
                (ib.read_mr(0), ib.read_mr(1))
            });
            handle_user_exception(tid, fault_ip, reason).await;
        }
        // UnknownSyscall
        3 => {
            let syscall_no = with_ipc_buffer(|ib| ib.read_mr(0));
            handle_unknown_syscall(tid, syscall_no).await;
        }
        _ => {
            seL4_DebugPutString("[lcl] Unknown fault type\n");
        }
    }
}
