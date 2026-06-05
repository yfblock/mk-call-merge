//! High-level seL4 system call wrappers.

use crate::{
    sys_null, ipc_buffer_addr, with_ipc_buffer, SYS_CALL, SYS_DEBUG_DUMP_SCHEDULER, SYS_DEBUG_HALT,
    SYS_DEBUG_PUT_CHAR, SYS_NBSEND, SYS_RECV, SYS_REPLY, SYS_REPLY_RECV,
    SYS_SEND, SYS_SET_TLS_BASE, SYS_SIGNAL, SYS_YIELD,
};

// ---------------------------------------------------------------------------
// Core IPC syscalls (2-register: tag + dest)
// ---------------------------------------------------------------------------

/// Blocking send to a capability (endpoint).
///
/// Returns `(tag, badge)` — the response `MessageInfo` word and the badge of
/// the receiving endpoint (0 if unbadged).
pub fn seL4_Send(dest: usize, info: usize) -> (usize, usize) {
    unsafe {
        let (_status, tag, badge) = crate::sys_send2(SYS_SEND, dest, info);
        (tag, badge)
    }
}

/// Non-blocking send to a capability.
pub fn seL4_NBSend(dest: usize, info: usize) -> (usize, usize) {
    unsafe {
        let (_status, tag, badge) = crate::sys_send2(SYS_NBSEND, dest, info);
        (tag, badge)
    }
}

/// Blocking call: send to an endpoint and wait for a reply.
///
/// Returns `(error, tag)` where `error == 0` means success.
/// The error code is extracted from the tag's label field (bits 12..).
pub fn seL4_Call(dest: usize, info: usize) -> (usize, usize) {
    unsafe {
        let (_status, tag, _badge) = crate::sys_send2(SYS_CALL, dest, info);
        // Kernel returns error in tag's label field. RAX is always 0 for Call.
        let error = tag >> 12;
        (error, tag)
    }
}

/// Send a reply to a pending call (implicit reply capability).
pub fn seL4_Reply(info: usize) {
    unsafe {
        crate::sys_send1(SYS_REPLY, info);
    }
}

/// Blocking receive from an endpoint.
///
/// Returns `(tag, badge)` and writes MR[0..3] from CPU registers into the IPC buffer
/// so that `with_ipc_buffer(|ib| ib.read_mr(i))` returns the correct values.
pub fn seL4_Recv(src: usize) -> (usize, usize) {
    let mut tag: usize;
    let mut badge: usize;
    let mut mr0: usize;
    let mut mr1: usize;
    let mut mr2: usize;
    let mut mr3: usize;
    unsafe {
        core::arch::asm!(
            "mov r14, rsp",
            "syscall",
            "mov rsp, r14",
            in("rdx") SYS_RECV,
            in("rdi") src,
            in("rsi") 0usize,
            lateout("rax") _,
            lateout("rdi") badge,
            lateout("rsi") tag,
            lateout("r10") mr0,
            lateout("r8")  mr1,
            lateout("r9")  mr2,
            lateout("r15") mr3,
            lateout("r12") _,
            lateout("r13") _,
            lateout("rcx") _,
            lateout("r11") _,
            lateout("r14") _,
            options(nostack),
        );
    }
    // Write MR[0..3] to the IPC buffer so read_mr() works
    with_ipc_buffer(|ib| {
        ib.write_mr(0, mr0);
        ib.write_mr(1, mr1);
        ib.write_mr(2, mr2);
        ib.write_mr(3, mr3);
    });
    (tag, badge)
}

/// Non-blocking receive from an endpoint.
pub fn seL4_NBRecv(src: usize) -> (usize, usize) {
    unsafe {
        let (tag, badge, _) = crate::sys_send2(crate::SYS_NBRECV, src, 0);
        (tag, badge)
    }
}

/// Blocking reply-and-receive (combined in one syscall).
pub fn seL4_ReplyRecv(dest: usize, info: usize, src: usize) -> (usize, usize) {
    let mut _badge_rax: usize;
    let mut tag: usize;
    let mut badge: usize;
    unsafe {
        core::arch::asm!(
            "mov r14, rsp",
            "syscall",
            "mov rsp, r14",
            in("rdx") SYS_REPLY_RECV,
            in("rdi") dest,
            in("rsi") info,
            in("r10") src,
            lateout("rax") _badge_rax,
            lateout("rdi") tag,
            lateout("rsi") badge,
            lateout("r10") _,
            lateout("r8")  _,
            lateout("r9")  _,
            lateout("r12") _,
            lateout("r13") _,
            lateout("r15") _,
            lateout("rcx") _,
            lateout("r11") _,
            lateout("r14") _,
            options(nostack),
        );
    }
    (tag, badge)
}

// ---------------------------------------------------------------------------
// Utility syscalls (0-register)
// ---------------------------------------------------------------------------

/// Yield the current thread's remaining timeslice.
pub fn seL4_Yield() {
    unsafe {
        sys_null(SYS_YIELD);
    }
}

/// Signal a notification object (non-blocking).
pub fn seL4_Signal(dest: usize) {
    unsafe {
        crate::sys_send1(SYS_SIGNAL, dest);
    }
}

/// Wait on a notification object (blocking).
///
/// Returns the badge of the notification that was signaled.
pub fn seL4_Wait(src: usize) -> usize {
    unsafe {
        let (badge, _) = crate::sys_send1(SYS_RECV, src);
        badge
    }
}

/// Output a single character to the kernel's debug serial port.
pub fn seL4_DebugPutChar(c: u8) {
    unsafe {
        core::arch::asm!(
            "mov r14, rsp",
            "syscall",
            "mov rsp, r14",
            in("rdx") SYS_DEBUG_PUT_CHAR,
            in("rdi") c as usize,
            lateout("rax") _,
            lateout("rsi") _,
            lateout("r10") _,
            lateout("r8")  _,
            lateout("r9")  _,
            lateout("r12") _,
            lateout("r13") _,
            lateout("r15") _,
            lateout("rcx") _,
            lateout("r11") _,
            lateout("r14") _,
            options(nostack),
        );
    }
}

/// Output a string to the kernel's debug serial port.
pub fn seL4_DebugPutString(s: &str) {
    for &b in s.as_bytes() {
        seL4_DebugPutChar(b);
    }
}

/// Dump the kernel scheduler state (debug).
pub fn seL4_DebugDumpScheduler() {
    unsafe {
        sys_null(SYS_DEBUG_DUMP_SCHEDULER);
    }
}

/// Halt the kernel and shut down the system.
pub fn seL4_DebugHalt() {
    unsafe {
        sys_null(SYS_DEBUG_HALT);
    }
}

/// Set the current thread's TLS base register (FS base on x86_64).
pub fn seL4_SetTLSBase(addr: usize) -> (usize, usize) {
    unsafe {
        let (status, badge, _) = crate::sys_send2(SYS_SET_TLS_BASE, addr, 0);
        (status, badge)
    }
}

// ---------------------------------------------------------------------------
// Capability invocation helpers
// ---------------------------------------------------------------------------
//
// seL4 capability invocations use `seL4_Call` to a CPtr. The MessageInfo
// `label` field selects the operation, and the `length` field tells the kernel
// how many message registers to read.
//
// Message registers (MRs) on x86_64:
//   CPU: r10=MR0, r8=MR1, r9=MR2, r15=MR3
//   IPC buffer msg[4..]: MR4+

/// Prepare the IPC buffer for a capability invocation and execute `seL4_Call`.
///
/// Returns 0 on success, or an error code on failure.
///
/// seL4 Call invocation reply convention:
/// - Success: reply tag label = 0
/// - Error: reply tag label = seL4 error code (non-zero)
fn cap_invoke(dest: usize, label: u32, mrs: &[usize]) -> usize {
    let total_mrs = mrs.len();
    // seL4 tag format: label at bits 12+, extraCaps at bits 7-8, length at bits 0-6
    let info = (label as usize) << 12 | total_mrs;

    // CPU registers carry MR0-MR3: r10=MR0, r8=MR1, r9=MR2, r15=MR3.
    // MR4-MR9 go into the IPC buffer msg[4..9].
    // MR10+ are written directly to the IPC buffer msg[10+].
    let r10 = if total_mrs > 0 { mrs[0] } else { 0 };
    let r8  = if total_mrs > 1 { mrs[1] } else { 0 };
    let r9  = if total_mrs > 2 { mrs[2] } else { 0 };
    let r15 = if total_mrs > 3 { mrs[3] } else { 0 };
    let mr4 = if total_mrs > 4  { mrs[4] } else { 0 };
    let mr5 = if total_mrs > 5  { mrs[5] } else { 0 };
    let mr6 = if total_mrs > 6  { mrs[6] } else { 0 };
    let mr7 = if total_mrs > 7  { mrs[7] } else { 0 };
    let mr8 = if total_mrs > 8  { mrs[8] } else { 0 };
    let mr9 = if total_mrs > 9  { mrs[9] } else { 0 };
    let buf_addr = ipc_buffer_addr() + 8; // msg[] starts at byte offset 8

    // Write MR10+ directly to the IPC buffer (seL4_syscall_with_buf only handles MR4-MR9).
    unsafe {
        for i in 10..total_mrs.min(120) {
            core::ptr::write_volatile((buf_addr + i * 8) as *mut usize, mrs[i]);
        }
    }

    let (_zero, tag, _badge, _mr0, _, _, _, _, _) = unsafe {
        crate::seL4_syscall_with_buf(
            crate::SYS_CALL, dest, info, r10, r8, r9, 0, 0, r15,
            buf_addr, mr4, mr5, mr6, mr7, mr8, mr9,
        )
    };
    tag >> 12
}

/// Like `cap_invoke` but also passes extra capabilities via the IPC buffer.
///
/// Extra caps go into caps_or_badges[0], caps_or_badges[1], ...
/// (kernel reads from bufferPtr[seL4_MsgMaxLength + 2 + i] = caps_or_badges[i])
fn cap_invoke_with_extra(
    dest: usize,
    label: u32,
    mrs: &[usize],
    extra_caps: &[usize],
) -> usize {
    let total_mrs = mrs.len();
    let info = (label as usize) << 12 | (extra_caps.len() << 7) | total_mrs;

    // CPU registers carry MR0-MR3. MR4-MR9 go into the IPC buffer.
    let r10 = if total_mrs > 0 { mrs[0] } else { 0 };
    let r8  = if total_mrs > 1 { mrs[1] } else { 0 };
    let r9  = if total_mrs > 2 { mrs[2] } else { 0 };
    let r15 = if total_mrs > 3 { mrs[3] } else { 0 };
    let mr4 = if total_mrs > 4 { mrs[4] } else { 0 };
    let mr5 = if total_mrs > 5 { mrs[5] } else { 0 };
    let mr6 = if total_mrs > 6 { mrs[6] } else { 0 };
    let mr7 = if total_mrs > 7 { mrs[7] } else { 0 };
    let mr8 = if total_mrs > 8 { mrs[8] } else { 0 };
    let mr9 = if total_mrs > 9 { mrs[9] } else { 0 };

    let cap0 = if extra_caps.len() > 0 { extra_caps[0] } else { 0 };
    let cap1 = if extra_caps.len() > 1 { extra_caps[1] } else { 0 };
    let cap2 = if extra_caps.len() > 2 { extra_caps[2] } else { 0 };
    let num_caps = extra_caps.len();
    let buf_addr = ipc_buffer_addr();

    let (_zero, tag, _badge, _mr0, _, _, _, _, _) = unsafe {
        crate::seL4_syscall_with_caps(
            crate::SYS_CALL, dest, info, r10, r8, r9, 0, 0, r15,
            buf_addr, mr4, mr5, mr6, mr7, mr8, mr9,
            num_caps, cap0, cap1, cap2,
        )
    };
    tag >> 12
}

// ---------------------------------------------------------------------------
// TCB (Thread Control Block) operations
// ---------------------------------------------------------------------------

/// Configure a TCB: set CSpace root, VSpace root, fault endpoint, and IPC
/// buffer.
pub fn seL4_TCB_Configure(
    tcb: usize,
    fault_ep: usize,
    cspace_root: usize,
    cspace_root_data: usize,
    vspace_root: usize,
    ipc_buffer_addr: usize,
    ipc_buffer_cap: usize,
) -> usize {
    cap_invoke_with_extra(
        tcb,
        5, // TCBConfigure label
        &[
            fault_ep,
            cspace_root_data,
            0, // vspace_root_data (unused)
            ipc_buffer_addr,
        ],
        &[cspace_root, vspace_root, ipc_buffer_cap],
    )
}

/// Write all registers for a TCB (set initial thread context).
///
/// The `reg_frame` slice contains register values in the kernel's
/// Read registers from a TCB.
///
/// `frameRegisters[]` order: FaultIP, RSP, FLAGS, RAX, RBX, RCX, RDX,
/// RSI, RDI, RBP, R8, R9, R10, R11, R12, R13, R14, R15.
///
/// MR layout: MR0 = suspend (with archFlags in upper bits), MR1 = count,
/// MR2.. = register values (output).
///
/// On x86_64, the first 4 message registers (MR0-MR3) are returned via CPU
/// registers (r10, r8, r9, r15). The remaining registers are in the IPC buffer.
pub fn seL4_TCB_ReadRegisters(
    tcb: usize,
    suspend: bool,
    arch_flags: u8,
    count: u8,
    reg_frame: &mut [usize],
) -> usize {
    let flags = (suspend as usize) | ((arch_flags as usize) << 8);
    let n = reg_frame.len().min(count as usize);
    let mrs = [flags, n as usize];

    // Use seL4_syscall_with_buf directly to capture CPU register outputs
    let info = (2usize << 12) | mrs.len(); // label=2 (TCBReadRegisters), length=2
    let buf_addr = ipc_buffer_addr() + 8; // msg[] starts at byte offset 8

    let (_zero, tag, _badge, mr0, mr1, mr2, mr3, _, _) = unsafe {
        crate::seL4_syscall_with_buf(
            crate::SYS_CALL, tcb, info,
            mrs[0], mrs[1], 0, 0, 0, 0, // MR0=flags, MR1=count, rest=0
            buf_addr, 0, 0, 0, 0, 0, 0,
        )
    };

    let error = tag >> 12;

    // The first 4 registers come from CPU registers (mr0=MR0, mr1=MR1, etc.)
    // The remaining registers come from the IPC buffer
    if n > 0 { reg_frame[0] = mr0; }
    if n > 1 { reg_frame[1] = mr1; }
    if n > 2 { reg_frame[2] = mr2; }
    if n > 3 { reg_frame[3] = mr3; }

    // Read remaining registers from IPC buffer
    crate::with_ipc_buffer(|ib| {
        for i in 4..n {
            reg_frame[i] = ib.read_mr(i);
        }
    });

    error
}

/// `frameRegisters[]` order: FaultIP, RSP, FLAGS, RAX, RBX, RCX, RDX,
/// RSI, RDI, RBP, R8, R9, R10, R11, R12, R13, R14, R15.
///
/// MR layout: MR0 = resume (with archFlags in upper bits), MR1 = count,
/// MR2.. = register values.
pub fn seL4_TCB_WriteRegisters(
    tcb: usize,
    resume: bool,
    arch_flags: u8,
    count: u8,
    reg_frame: &[usize],
) -> usize {
    // Kernel decode: flags = getSyscallArg(0), w = getSyscallArg(1)
    // flags & BIT(0) = resume, w = number of registers to write.
    // Then getSyscallArg(2+i) = frameRegisters[i].
    let flags = (resume as usize) | ((arch_flags as usize) << 8);
    let n = reg_frame.len().min(count as usize);
    let mut mrs = [0usize; 2 + 20]; // MR0=flags, MR1=count, MR2..=regs (up to 20)
    mrs[0] = flags;
    mrs[1] = n;
    mrs[2..2 + n].copy_from_slice(&reg_frame[..n]);
    cap_invoke(tcb, 3, &mrs[..2 + n]) // TCBWriteRegisters label = 3
}

/// Set scheduling parameters for a TCB.
/// Suspend a TCB.
pub fn seL4_TCB_Suspend(tcb: usize) -> usize {
    cap_invoke(tcb, 11, &[]) // TCBSuspend label = 11
}

/// Resume a suspended TCB.
pub fn seL4_TCB_Resume(tcb: usize) -> usize {
    cap_invoke(tcb, 12, &[]) // TCBResume label = 12
}

pub fn seL4_TCB_SetSchedParams(
    tcb: usize,
    authority: usize,
    mcp: u8,
    priority: u8,
) -> usize {
    cap_invoke_with_extra(
        tcb,
        8, // TCBSetSchedParams label
        &[mcp as usize, priority as usize],
        &[authority],
    )
}

/// Bind a notification object to a TCB (for fault or signal delivery).
pub fn seL4_TCB_BindNotification(tcb: usize, ntfn: usize) -> usize {
    cap_invoke_with_extra(tcb, 13, &[], &[ntfn]) // TCBBindNotification label
}

/// Unbind a notification from a TCB.
pub fn seL4_TCB_UnbindNotification(tcb: usize) -> usize {
    cap_invoke(tcb, 14, &[]) // TCBUnbindNotification label
}

/// Set the TLS base address for a TCB.
pub fn seL4_TCB_SetTLSBase(tcb: usize, tls_base: usize) -> usize {
    cap_invoke(tcb, 15, &[tls_base]) // TCBSetTLSBase label
}

// ---------------------------------------------------------------------------
// CNode (Capability space) operations
// ---------------------------------------------------------------------------

/// Copy a capability from one slot to another.
pub fn seL4_CNode_Copy(
    cnode: usize,
    dest_index: usize,
    dest_depth: u8,
    src_root: usize,
    src_index: usize,
    src_depth: u8,
    rights: usize,
) -> usize {
    cap_invoke_with_extra(
        cnode,
        20, // CNodeCopy label
        &[
            dest_index,
            dest_depth as usize,
            src_index,
            src_depth as usize,
            rights,
        ],
        &[src_root],
    )
}

/// Mint a capability (copy with a new badge or guard).
pub fn seL4_CNode_Mint(
    cnode: usize,
    dest_index: usize,
    dest_depth: u8,
    src_root: usize,
    src_index: usize,
    src_depth: u8,
    rights: usize,
    badge: usize,
) -> usize {
    cap_invoke_with_extra(
        cnode,
        21, // CNodeMint label
        &[
            dest_index,
            dest_depth as usize,
            src_index,
            src_depth as usize,
            rights,
            badge,
        ],
        &[src_root],
    )
}

/// Delete a capability from a slot.
pub fn seL4_CNode_Delete(cnode: usize, index: usize, depth: u8) -> usize {
    cap_invoke(cnode, 18, &[index, depth as usize]) // CNodeDelete label
}

/// Revoke all capabilities derived from a slot.
pub fn seL4_CNode_Revoke(cnode: usize, index: usize, depth: u8) -> usize {
    cap_invoke(cnode, 17, &[index, depth as usize]) // CNodeRevoke label
}

// ---------------------------------------------------------------------------
// Untyped (physical memory) operations
// ---------------------------------------------------------------------------

/// Retype an untyped memory region into kernel objects.
pub fn seL4_Untyped_Retype(
    untyped: usize,
    obj_type: usize,
    size_bits: usize,
    root_cnode: usize,
    node_index: usize,
    node_depth: usize,
    node_offset: usize,
    num_objects: usize,
) -> usize {
    // root_cnode is passed as an extra cap (caps_or_badges[0]), not as an MR.
    // MRs: [obj_type, size_bits, node_index, node_depth, node_offset, num_objects]
    cap_invoke_with_extra(
        untyped,
        1, // UntypedRetype label
        &[
            obj_type,
            size_bits,
            node_index,
            node_depth,
            node_offset,
            num_objects,
        ],
        &[root_cnode],
    )
}

// ---------------------------------------------------------------------------
// Frame (physical page) operations
// ---------------------------------------------------------------------------

/// Map a frame into a virtual address space.
pub fn seL4_Frame_Map(
    frame: usize,
    vspace: usize,
    vaddr: usize,
    rights: usize,
    attr: usize,
) -> usize {
    cap_invoke_with_extra(frame, 41, &[vaddr, rights, attr], &[vspace]) // X86PageMap label
}

/// Unmap a frame from all address spaces.
pub fn seL4_Frame_Unmap(frame: usize) -> usize {
    cap_invoke(frame, 42, &[]) // X86PageUnmap label
}

// ---------------------------------------------------------------------------
// Page table operations
// ---------------------------------------------------------------------------

/// Map a page table into a parent page table.
pub fn seL4_PageTable_Map(pt: usize, vspace: usize, vaddr: usize, attr: usize) -> usize {
    cap_invoke_with_extra(pt, 37, &[vaddr, attr], &[vspace]) // X86PageTableMap label
}

/// Unmap a page table.
pub fn seL4_PageTable_Unmap(pt: usize) -> usize {
    cap_invoke(pt, 38, &[]) // X86PageTableUnmap label
}

// ---------------------------------------------------------------------------
// Page Directory operations
// ---------------------------------------------------------------------------

/// Map a page directory into a PDPT or PML4.
pub fn seL4_PageDirectory_Map(
    pd: usize,
    vspace: usize,
    vaddr: usize,
    attr: usize,
) -> usize {
    cap_invoke_with_extra(pd, 35, &[vaddr, attr], &[vspace]) // X86PageDirectoryMap label
}

/// Unmap a page directory.
pub fn seL4_PageDirectory_Unmap(pd: usize) -> usize {
    cap_invoke(pd, 36, &[]) // X86PageDirectoryUnmap label
}

// ---------------------------------------------------------------------------
// PDPT operations
// ---------------------------------------------------------------------------

/// Map a PDPT into a PML4.
pub fn seL4_PDPT_Map(pdpt: usize, vspace: usize, vaddr: usize, attr: usize) -> usize {
    cap_invoke_with_extra(pdpt, 33, &[vaddr, attr], &[vspace]) // X86PDPTMap label
}

/// Unmap a PDPT.
pub fn seL4_PDPT_Unmap(pdpt: usize) -> usize {
    cap_invoke(pdpt, 34, &[]) // X86PDPTUnmap label
}

// ---------------------------------------------------------------------------
// ASID operations
// ---------------------------------------------------------------------------

/// Assign an ASID to a VSpace.
pub fn seL4_ASIDPool_Assign(asid_pool: usize, vspace: usize) -> usize {
    cap_invoke(asid_pool, 46, &[vspace]) // X86ASIDPoolAssign label
}

/// Create an ASID pool from untyped memory.
pub fn seL4_ASIDControl_MakePool(
    asid_control: usize,
    untyped: usize,
    root_cnode: usize,
    index: usize,
    depth: usize,
) -> usize {
    cap_invoke_with_extra(
        asid_control,
        45, // X86ASIDControlMakePool label
        &[index, depth],
        &[untyped, root_cnode],
    )
}

// ---------------------------------------------------------------------------
// IRQ operations
// ---------------------------------------------------------------------------

/// Get an IRQ handler capability for a specific IRQ line.
pub fn seL4_IRQControl_Get(
    irq_control: usize,
    irq: usize,
    root_cnode: usize,
    index: usize,
    depth: usize,
) -> usize {
    cap_invoke_with_extra(irq_control, 26, &[irq, index, depth], &[root_cnode]) // IRQIssueIRQHandler
}

/// Set the notification that an IRQ handler will signal when the IRQ fires.
pub fn seL4_IRQHandler_SetNotification(irq_handler: usize, notification: usize) -> usize {
    cap_invoke(irq_handler, 28, &[notification]) // IRQSetIRQHandler
}

/// Acknowledge an IRQ (re-enable it after handling).
pub fn seL4_IRQHandler_Ack(irq_handler: usize) -> usize {
    cap_invoke(irq_handler, 27, &[]) // IRQAckIRQ
}

// ---------------------------------------------------------------------------
// x86 I/O Port operations
// ---------------------------------------------------------------------------

/// Issue (create) an I/O port capability for a port range.
///
/// The new capability is stored in `dest_index` of the `dest_cnode`.
pub fn seL4_X86_IOPortControl_Issue(
    io_port_control: usize,
    first_port: u16,
    last_port: u16,
    dest_cnode: usize,
    dest_index: usize,
    dest_depth: u8,
) -> usize {
    cap_invoke_with_extra(
        io_port_control,
        47, // X86IOPortControlIssue
        &[first_port as usize, last_port as usize, dest_index, dest_depth as usize],
        &[dest_cnode],
    )
}

/// Write 8 bits to an I/O port.
pub fn seL4_X86_IOPort_Out8(io_port_cap: usize, port: u16, data: u8) -> usize {
    cap_invoke(io_port_cap, 51, &[port as usize, data as usize]) // X86IOPortOut8
}

/// Write 16 bits to an I/O port.
pub fn seL4_X86_IOPort_Out16(io_port_cap: usize, port: u16, data: u16) -> usize {
    cap_invoke(io_port_cap, 52, &[port as usize, data as usize]) // X86IOPortOut16
}

/// Write 32 bits to an I/O port.
pub fn seL4_X86_IOPort_Out32(io_port_cap: usize, port: u16, data: u32) -> usize {
    cap_invoke(io_port_cap, 53, &[port as usize, data as usize]) // X86IOPortOut32
}
