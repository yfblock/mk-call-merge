//! Multi-threaded IPC performance benchmark.
//!
//! Compares Endpoint (Call/ReplyRecv) vs Notification (Signal/Wait) round-trip
//! latency between two threads.

use sel4_sys::*;
use crate::print;
use crate::slot::SLOT_MANAGER;

const PAGE_SIZE: usize = 4096;
const ITERATIONS: u64 = 10000;
const CNODE_DEPTH: usize = 64;
const STACK_VADDR: usize = 0x500000;
const IPC_BUF_VADDR: usize = 0x501000;

/// Read the x86_64 TSC (Time Stamp Counter).
#[inline]
pub fn rdtsc() -> u64 {
    unsafe {
        let lo: u32;
        let hi: u32;
        core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi);
        ((hi as u64) << 32) | (lo as u64)
    }
}

unsafe extern "C" {
    fn worker_ep_entry();
    fn worker_ntfn_entry();
}

/// Run multi-threaded IPC benchmark: Endpoint (Call/ReplyRecv) vs Notification (Signal/Wait).
pub fn run(bi: &BootInfo) {
    // --- Allocate capability slots ---
    let (tcb_slot, ep_slot, ntfn_slot, stack_frame_slot, ipc_frame_slot);
    {
        let mut sm = SLOT_MANAGER.lock();
        tcb_slot = sm.alloc().unwrap();
        ep_slot = sm.alloc().unwrap();
        ntfn_slot = sm.alloc().unwrap();
        stack_frame_slot = sm.alloc().unwrap();
        ipc_frame_slot = sm.alloc().unwrap();
    }

    let (untyped_slot, _untyped_size) = bi.find_free_untyped(12)
        .expect("No untyped >= 4KB found for TCB/frame allocation");
    seL4_DebugPutString("[mt-bench] setting up kernel objects ...\n");

    // --- Create all kernel objects ---
    for (name, obj_type, size_bits, slot) in [
        ("TCB", ObjectType::TCB as usize, ObjectType::TCB.size_bits(), tcb_slot),
        ("stack frame", ObjectType::Frame4K as usize, ObjectType::Frame4K.size_bits(), stack_frame_slot),
        ("IPC buf frame", ObjectType::Frame4K as usize, ObjectType::Frame4K.size_bits(), ipc_frame_slot),
        ("Endpoint", ObjectType::Endpoint as usize, ObjectType::Endpoint.size_bits(), ep_slot),
        ("Notification", ObjectType::Notification as usize, ObjectType::Notification.size_bits(), ntfn_slot),
    ] {
        let err = seL4_Untyped_Retype(untyped_slot, obj_type, size_bits,
            init_slots::CNODE, init_slots::CNODE, CNODE_DEPTH, slot, 1);
        if err != 0 {
            seL4_DebugPutString("  FAILED creating ");
            seL4_DebugPutString(name);
            seL4_DebugPutString(" err=");
            print::put_u64(err as u64);
            seL4_DebugPutChar(b'\n');
            return;
        }
    }
    seL4_DebugPutString("  kernel objects OK\n");

    // --- Create and map a PageTable for vaddr 0x400000 ---
    let pt_slot = { SLOT_MANAGER.lock().alloc().unwrap() };
    let err = seL4_Untyped_Retype(untyped_slot, ObjectType::PageTable as usize,
        ObjectType::PageTable.size_bits(), init_slots::CNODE, init_slots::CNODE,
        CNODE_DEPTH, pt_slot, 1);
    if err != 0 { seL4_DebugPutString("  FAILED creating PageTable\n"); return; }
    let err = seL4_PageTable_Map(pt_slot, init_slots::VSPACE, 0x400000, 0);
    if err != 0 { seL4_DebugPutString("  FAILED mapping PageTable\n"); return; }

    // --- Map frames into VSpace ---
    let err = seL4_Frame_Map(stack_frame_slot, init_slots::VSPACE, STACK_VADDR, CapRights::ALL.bits(), 0);
    if err != 0 { seL4_DebugPutString("  FAILED mapping stack\n"); return; }
    let err = seL4_Frame_Map(ipc_frame_slot, init_slots::VSPACE, IPC_BUF_VADDR, CapRights::ALL.bits(), 0);
    if err != 0 { seL4_DebugPutString("  FAILED mapping IPC buf\n"); return; }
    seL4_DebugPutString("  page mappings OK\n");

    // --- Initialize worker IPC buffer ---
    unsafe {
        let worker_ipc = &mut *(IPC_BUF_VADDR as *mut IpcBuffer);
        *worker_ipc = IpcBuffer::new();
        worker_ipc.set_receive_slot(init_slots::CNODE, 0, CNODE_DEPTH);
    }

    // --- Configure TCB ---
    let err = seL4_TCB_Configure(tcb_slot, 0, init_slots::CNODE, 0,
        init_slots::VSPACE, IPC_BUF_VADDR, ipc_frame_slot);
    if err != 0 {
        seL4_DebugPutString("  FAILED TCB_Configure err=");
        print::put_u64(err as u64);
        seL4_DebugPutChar(b'\n');
        return;
    }
    let err = seL4_TCB_SetSchedParams(tcb_slot, init_slots::TCB, 255, 255);
    if err != 0 {
        seL4_DebugPutString("  FAILED SetSchedParams err=");
        print::put_u64(err as u64);
        seL4_DebugPutChar(b'\n');
        return;
    }
    seL4_DebugPutString("  TCB configured OK\n");

    // =========================================================================
    // Endpoint Benchmark: Call / ReplyRecv round-trip
    // =========================================================================

    // frameRegisters[] order: FaultIP=0, RSP=1, FLAGS=2, RAX=3, RBX=4, RCX=5,
    // RDX=6, RSI=7, RDI=8, RBP=9, R8=10, R9=11, R10=12, R11=13, R12=14,
    // R13=15, R14=16, R15=17
    seL4_DebugPutString("[mt-bench] starting Endpoint worker ... ");
    {
        let mut regs = [0usize; 18];
        regs[0]  = worker_ep_entry as usize;           // FaultIP (RIP)
        regs[1]  = (STACK_VADDR + PAGE_SIZE) as usize; // RSP
        regs[2]  = 0x202;                              // FLAGS
        regs[8]  = ep_slot as usize;                   // RDI = endpoint cap
        regs[12] = ep_slot as usize;                   // R10 = endpoint src
        regs[14] = ep_slot as usize;                   // R12 = preserved cap slot
        let err = seL4_TCB_WriteRegisters(tcb_slot, true, 0, 18, &regs);
        if err != 0 {
            seL4_DebugPutString("FAILED (WriteRegisters)\n");
            return;
        }
    }
    seL4_DebugPutString("OK\n");

    // Warmup
    for _ in 0..100 {
        seL4_Call(ep_slot, 0);
    }

    seL4_DebugPutString("[mt-bench] benchmarking Endpoint Call/ReplyRecv ...\n");
    let t0 = rdtsc();
    for _ in 0..ITERATIONS {
        seL4_Call(ep_slot, 0);
    }
    let ep_cycles = (rdtsc() - t0) / ITERATIONS;

    // =========================================================================
    // Notification Benchmark: Signal / Wait round-trip
    // =========================================================================

    seL4_DebugPutString("[mt-bench] switching worker to Notification ... ");
    {
        let mut regs = [0usize; 18];
        regs[0]  = worker_ntfn_entry as usize;         // FaultIP (RIP)
        regs[1]  = (STACK_VADDR + PAGE_SIZE) as usize; // RSP
        regs[2]  = 0x202;                              // FLAGS
        regs[8]  = ntfn_slot as usize;                 // RDI = notification cap
        regs[14] = ntfn_slot as usize;                 // R12 = preserved cap slot
        let err = seL4_TCB_WriteRegisters(tcb_slot, true, 0, 18, &regs);
        if err != 0 {
            seL4_DebugPutString("FAILED\n");
            return;
        }
    }
    seL4_DebugPutString("OK\n");

    // Warmup
    for _ in 0..100 {
        seL4_Signal(ntfn_slot);
    }

    seL4_DebugPutString("[mt-bench] benchmarking Notification Signal/Wait ...\n");
    let t0 = rdtsc();
    for _ in 0..ITERATIONS {
        seL4_Signal(ntfn_slot);
    }
    let ntfn_cycles = (rdtsc() - t0) / ITERATIONS;

    // =========================================================================
    // Results
    // =========================================================================

    seL4_DebugPutString("\n=== Multi-threaded IPC Benchmark ===\n");
    seL4_DebugPutString("  Iterations: ");
    print::put_u64(ITERATIONS);
    seL4_DebugPutString("\n\n");

    seL4_DebugPutString("  Endpoint (Call/ReplyRecv):     ");
    print::put_u64(ep_cycles);
    seL4_DebugPutString(" cycles/round-trip\n");

    seL4_DebugPutString("  Notification (Signal/Wait):    ");
    print::put_u64(ntfn_cycles);
    seL4_DebugPutString(" cycles/round-trip\n");

    seL4_DebugPutString("\n  Endpoint overhead vs Notification: ");
    if ep_cycles > ntfn_cycles {
        print::put_u64(ep_cycles - ntfn_cycles);
        seL4_DebugPutString(" cycles");
    } else {
        seL4_DebugPutChar(b'-');
        print::put_u64(ntfn_cycles - ep_cycles);
        seL4_DebugPutString(" cycles");
    }

    if ntfn_cycles > 0 {
        seL4_DebugPutString("\n  Endpoint/Notification ratio: ");
        let ratio_x10 = (ep_cycles * 10) / ntfn_cycles;
        print::put_u64(ratio_x10 / 10);
        seL4_DebugPutChar(b'.');
        print::put_u64(ratio_x10 % 10);
        seL4_DebugPutString("x");
    }
    seL4_DebugPutString("\n=====================================\n\n");
}
