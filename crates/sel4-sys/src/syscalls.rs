//! High-level seL4 system call wrappers.
//!
//! ... (see module doc)

use crate::{
    sys_null, with_ipc_buffer, SYS_CALL, SYS_DEBUG_DUMP_SCHEDULER,
    SYS_DEBUG_PUT_CHAR, SYS_NBSEND, SYS_NBWAIT, SYS_POLL, SYS_RECV, SYS_REPLY, SYS_SEND,
    SYS_SET_TLS_BASE, SYS_YIELD,
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
        let (_status, tag, badge) = crate::sys_send2(SYS_SEND, info, dest);
        (tag, badge)
    }
}

/// Non-blocking send to a capability.
pub fn seL4_NBSend(dest: usize, info: usize) -> (usize, usize) {
    unsafe {
        let (_status, tag, badge) = crate::sys_send2(SYS_NBSEND, info, dest);
        (tag, badge)
    }
}

/// Blocking call: send to an endpoint and wait for a reply.
///
/// Returns `(status, tag)` where `status == 0` means success.
pub fn seL4_Call(dest: usize, info: usize) -> (usize, usize) {
    unsafe {
        let (status, tag, _badge) = crate::sys_send2(SYS_CALL, info, dest);
        (status, tag)
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
/// Returns `(tag, badge)`.
pub fn seL4_Recv(src: usize) -> (usize, usize) {
    unsafe {
        let (tag, badge, _) = crate::sys_send2(SYS_RECV, src, 0);
        (tag, badge)
    }
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
    let mut status: usize;
    let mut badge: usize;
    unsafe {
        core::arch::asm!(
            "mov r14, rsp",
            "syscall",
            "mov rsp, r14",
            in("rdx") -2isize,  // ReplyRecv syscall number
            in("rdi") info,
            in("rsi") dest,
            in("r10") src,
            lateout("rax") status,
            lateout("rsi") badge,
            lateout("rcx") _,
            lateout("r11") _,
            lateout("r14") _,
            options(nomem, nostack),
        );
    }
    (status, badge)
}

// ---------------------------------------------------------------------------
// Notification syscalls
// ---------------------------------------------------------------------------

/// Non-blocking wait on a notification object.
///
/// Returns `(state, badge)`. If `state != 0`, the notification was signaled.
pub fn seL4_NBWait(ntfn: usize) -> (usize, usize) {
    unsafe {
        let (state, badge, _) = crate::sys_send2(SYS_NBWAIT, ntfn, 0);
        (state, badge)
    }
}

/// Poll an endpoint or notification object without blocking.
///
/// Returns `(tag_or_state, badge)`.
pub fn seL4_Poll(obj: usize) -> (usize, usize) {
    let mut tag: usize;
    let mut badge: usize;
    unsafe {
        core::arch::asm!(
            "mov r14, rsp",
            "syscall",
            "mov rsp, r14",
            in("rdx") SYS_POLL,
            in("rdi") obj,
            lateout("rax") tag,
            lateout("rsi") badge,
            lateout("rcx") _,
            lateout("r11") _,
            lateout("r14") _,
            options(nomem, nostack),
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

/// Output a single character to the kernel's debug serial port.
pub fn seL4_DebugPutChar(c: u8) {
    unsafe {
        core::arch::asm!(
            "mov r14, rsp",
            "syscall",
            "mov rsp, r14",
            in("rdx") SYS_DEBUG_PUT_CHAR,
            in("rdi") c as usize,
            lateout("rcx") _,
            lateout("r11") _,
            lateout("r14") _,
            options(nomem, nostack),
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
// Message registers (MRs) are laid out as follows:
//   MR0 = rdi (used as the tag for Call)
//   MR1 = rsi (destination CPtr for Call)
//   MR2..MR5 = r10, r8, r9, r12
//   MR6+ = IPC buffer msg[] array
//
// The `build_call` helper writes MRs beyond the first 2 into the IPC buffer
// and performs the call.

/// Prepare the IPC buffer for a capability invocation and execute `seL4_Call`.
///
/// Returns the raw status word (0 = success).
fn cap_invoke(dest: usize, label: u32, mrs: &[usize]) -> usize {
    // Build the MessageInfo word: (label << 12) | (length << 7)
    let info = (label as usize) << 12 | (mrs.len() << 7);

    // Write MRs into the IPC buffer (after the tag which is MR0 and dest
    // which is MR1). Actually, for seL4_Call, the first two message registers
    // are the tag (MR0) and the destination (MR1). The kernel reads MR2+ from
    // the IPC buffer when the length exceeds what fits in registers.
    //
    // For x86_64, MR0..MR5 can be passed in registers. MR6+ must use the IPC
    // buffer. Since we pass the tag and dest as separate operands to the
    // syscall, we need to write the remaining message words to the IPC
    // buffer starting at msg[0].
    if !mrs.is_empty() {
        with_ipc_buffer(|ipc_buf| {
            for (i, &mr) in mrs.iter().enumerate() {
                ipc_buf.write_mr(i, mr);
            }
        });
    }

    // Execute the call
    let (status, _tag) = seL4_Call(dest, info);
    status
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
    cap_invoke(
        tcb,
        1, // TCBConfigure label
        &[
            fault_ep,
            cspace_root,
            cspace_root_data,
            vspace_root,
            ipc_buffer_addr,
            ipc_buffer_cap,
        ],
    )
}

/// Write all registers for a TCB (set initial thread context).
pub fn seL4_TCB_WriteRegisters(
    tcb: usize,
    resume: bool,
    arch_flags: u8,
    count: u8,
    reg_frame: &[usize],
) -> usize {
    with_ipc_buffer(|ipc_buf| {
        // Write the header: resume, arch_flags, count
        ipc_buf.write_mr(0, resume as usize);
        ipc_buf.write_mr(1, arch_flags as usize);
        ipc_buf.write_mr(2, count as usize);
        // Write register values
        for (i, &val) in reg_frame.iter().enumerate() {
            ipc_buf.write_mr(3 + i, val);
        }
    });
    let info = (2usize << 12) | ((3 + reg_frame.len()) << 7);
    let (status, _tag) = seL4_Call(tcb, info);
    status
}

/// Set scheduling parameters for a TCB.
pub fn seL4_TCB_SetSchedParams(
    tcb: usize,
    authority: usize,
    mcp: u8,
    priority: u8,
) -> usize {
    cap_invoke(
        tcb,
        4, // TCBSetSchedParams label
        &[authority, mcp as usize, priority as usize],
    )
}

/// Bind a notification object to a TCB (for fault or signal delivery).
pub fn seL4_TCB_BindNotification(tcb: usize, ntfn: usize) -> usize {
    cap_invoke(tcb, 5, &[ntfn]) // TCBBindNotification label
}

/// Unbind a notification from a TCB.
pub fn seL4_TCB_UnbindNotification(tcb: usize) -> usize {
    cap_invoke(tcb, 6, &[]) // TCBUnbindNotification label
}

/// Set the TLS base address for a TCB.
pub fn seL4_TCB_SetTLSBase(tcb: usize, tls_base: usize) -> usize {
    cap_invoke(tcb, 7, &[tls_base]) // TCBSetTLSBase label
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
    cap_invoke(
        cnode,
        1, // CNodeCopy label
        &[
            dest_index,
            dest_depth as usize,
            src_root,
            src_index,
            src_depth as usize,
            rights,
        ],
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
    cap_invoke(
        cnode,
        2, // CNodeMint label
        &[
            dest_index,
            dest_depth as usize,
            src_root,
            src_index,
            src_depth as usize,
            rights,
            badge,
        ],
    )
}

/// Delete a capability from a slot.
pub fn seL4_CNode_Delete(cnode: usize, index: usize, depth: u8) -> usize {
    cap_invoke(cnode, 5, &[index, depth as usize]) // CNodeDelete label
}

/// Revoke all capabilities derived from a slot.
pub fn seL4_CNode_Revoke(cnode: usize, index: usize, depth: u8) -> usize {
    cap_invoke(cnode, 6, &[index, depth as usize]) // CNodeRevoke label
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
    cap_invoke(
        untyped,
        1, // UntypedRetype label
        &[
            obj_type,
            size_bits,
            root_cnode,
            node_index,
            node_depth,
            node_offset,
            num_objects,
        ],
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
    cap_invoke(frame, 0, &[vspace, vaddr, rights, attr]) // X86PageMap label
}

/// Unmap a frame from all address spaces.
pub fn seL4_Frame_Unmap(frame: usize) -> usize {
    cap_invoke(frame, 1, &[]) // X86PageUnmap label
}

// ---------------------------------------------------------------------------
// Page table operations
// ---------------------------------------------------------------------------

/// Map a page table into a parent page table.
pub fn seL4_PageTable_Map(pt: usize, vspace: usize, vaddr: usize, attr: usize) -> usize {
    cap_invoke(pt, 0, &[vspace, vaddr, attr]) // X86PageTableMap label
}

/// Unmap a page table.
pub fn seL4_PageTable_Unmap(pt: usize) -> usize {
    cap_invoke(pt, 1, &[]) // X86PageTableUnmap label
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
    cap_invoke(pd, 0, &[vspace, vaddr, attr]) // X86PageDirectoryMap label
}

/// Unmap a page directory.
pub fn seL4_PageDirectory_Unmap(pd: usize) -> usize {
    cap_invoke(pd, 1, &[]) // X86PageDirectoryUnmap label
}

// ---------------------------------------------------------------------------
// PDPT operations
// ---------------------------------------------------------------------------

/// Map a PDPT into a PML4.
pub fn seL4_PDPT_Map(pdpt: usize, vspace: usize, vaddr: usize, attr: usize) -> usize {
    cap_invoke(pdpt, 0, &[vspace, vaddr, attr]) // X86PDPTMap label
}

/// Unmap a PDPT.
pub fn seL4_PDPT_Unmap(pdpt: usize) -> usize {
    cap_invoke(pdpt, 1, &[]) // X86PDPTUnmap label
}

// ---------------------------------------------------------------------------
// ASID operations
// ---------------------------------------------------------------------------

/// Assign an ASID to a VSpace.
pub fn seL4_ASIDPool_Assign(asid_pool: usize, vspace: usize) -> usize {
    cap_invoke(asid_pool, 0, &[vspace]) // X86ASIDPoolAssign label
}

/// Create an ASID pool from untyped memory.
pub fn seL4_ASIDControl_MakePool(
    asid_control: usize,
    untyped: usize,
    root_cnode: usize,
    index: usize,
    depth: usize,
) -> usize {
    cap_invoke(
        asid_control,
        0, // X86ASIDControlMakePool label
        &[untyped, root_cnode, index, depth],
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
    cap_invoke(irq_control, 0, &[irq, root_cnode, index, depth])
}

/// Set the notification that an IRQ handler will signal when the IRQ fires.
pub fn seL4_IRQHandler_SetNotification(irq_handler: usize, notification: usize) -> usize {
    cap_invoke(irq_handler, 1, &[notification])
}

/// Acknowledge an IRQ (re-enable it after handling).
pub fn seL4_IRQHandler_Ack(irq_handler: usize) -> usize {
    cap_invoke(irq_handler, 0, &[])
}
