//! Comprehensive tests for the sel4-sys crate.
//!
//! These tests are designed to run inside an seL4 root task on QEMU.
//! They verify type layouts, encoding/decoding, constants, and runtime syscalls.

use crate::types::*;
use crate::ipc_buffer::IpcBuffer;
use crate::error::{self, Error};
use crate::syscalls::*;
use crate::*;

use alloc::string::ToString;
use core::mem::size_of;

// ---------------------------------------------------------------------------
// Test infrastructure
// ---------------------------------------------------------------------------

struct TestCase {
    name: &'static str,
    func: fn() -> bool,
}

macro_rules! test {
    ($name:expr, $func:expr) => {
        TestCase { name: $name, func: $func }
    };
}

/// Run all sel4-sys tests. Returns `(passed, failed)`.
pub fn run_sel4_sys_tests() -> (usize, usize) {
    let tests: &[TestCase] = &[
        // === A. MessageInfo ===
        test!("MessageInfo: zero", test_mi_zero),
        test!("MessageInfo: round-trip", test_mi_roundtrip),
        test!("MessageInfo: max values", test_mi_max),
        test!("MessageInfo: from_word(0)", test_mi_from_word_zero),
        test!("MessageInfo: from_word manual", test_mi_from_word_manual),
        test!("MessageInfo: Builder == new", test_mi_builder),
        test!("MessageInfo: Builder empty", test_mi_builder_empty),
        test!("MessageInfo: size", test_mi_size),
        // === B. CNodeCapData ===
        test!("CNodeCapData: round-trip", test_cn_roundtrip),
        test!("CNodeCapData: zero", test_cn_zero),
        test!("CNodeCapData: max fields", test_cn_max),
        test!("CNodeCapData: large guard", test_cn_large_guard),
        test!("CNodeCapData: from_word", test_cn_from_word),
        // === C. CapRights ===
        test!("CapRights: constants", test_cr_constants),
        test!("CapRights: default", test_cr_default),
        test!("CapRights: from_bits", test_cr_from_bits),
        test!("CapRights: custom combo", test_cr_custom),
        // === D. VmAttributes ===
        test!("VmAttributes: default", test_vm_default),
        test!("VmAttributes: with_pat", test_vm_pat),
        test!("VmAttributes: write_through", test_vm_wt),
        test!("VmAttributes: cache_disabled", test_vm_cd),
        test!("VmAttributes: combined", test_vm_combined),
        // === E. ObjectType ===
        test!("ObjectType: all size_bits", test_ot_all),
        test!("ObjectType: variable size", test_ot_variable),
        // === F. UserContext ===
        test!("UserContext: default", test_uc_default),
        test!("UserContext: set_ip/sp", test_uc_ip_sp),
        test!("UserContext: gpr mapping", test_uc_gpr),
        test!("UserContext: as_bytes len", test_uc_bytes),
        test!("UserContext: IPC round-trip", test_uc_ipc_roundtrip),
        // === G. FaultType ===
        test!("FaultType: all variants", test_ft_all),
        test!("FaultType: unknown fallback", test_ft_unknown),
        // === H. Error ===
        test!("Error: all known codes", test_err_all),
        test!("Error: unknown fallback", test_err_unknown),
        test!("Error: is_ok/is_err", test_err_ok_err),
        test!("Error: Display", test_err_display),
        test!("Error: check()", test_err_check),
        // === I. IPC Buffer ===
        test!("IpcBuffer: new tag=0", test_ipc_tag),
        test!("IpcBuffer: mr read/write", test_ipc_mr),
        test!("IpcBuffer: mr bounds", test_ipc_mr_bounds),
        test!("IpcBuffer: receive slot", test_ipc_recv_slot),
        test!("IpcBuffer: UserContext rw", test_ipc_user_ctx),
        test!("IpcBuffer: write/read_words", test_ipc_words),
        test!("IpcBuffer: size", test_ipc_size),
        // === J. Constants ===
        test!("Consts: syscall values", test_const_syscall_vals),
        test!("Consts: syscall distinct", test_const_syscall_distinct),
        test!("Consts: init_slots", test_const_init_slots),
        test!("Consts: IPC buffer consts", test_const_ipc),
        // === K. Runtime syscalls ===
        test!("Syscall: DebugPutString", test_rt_putstring),
        test!("Syscall: DebugDumpScheduler", test_rt_dumpsched),
        test!("Syscall: Yield", test_rt_yield),
        // === L. IPC Buffer TLS ===
        test!("TLS: with_ipc_buffer", test_tls_ipc),
    ];

    let mut passed = 0usize;
    let mut failed = 0usize;

    for t in tests {
        seL4_DebugPutString("  TEST: ");
        seL4_DebugPutString(t.name);
        seL4_DebugPutString(" ... ");
        let ok = (t.func)();
        if ok {
            seL4_DebugPutString("PASSED\n");
            passed += 1;
        } else {
            seL4_DebugPutString("FAILED\n");
            failed += 1;
        }
    }

    (passed, failed)
}

// ===========================================================================
// A. MessageInfo tests
// ===========================================================================

fn test_mi_zero() -> bool {
    let mi = MessageInfo::new(0, 0, 0);
    mi.word() == 0 && mi.label() == 0 && mi.length() == 0 && mi.extra_caps() == 0
}

fn test_mi_roundtrip() -> bool {
    let mi = MessageInfo::new(42, 7, 3);
    mi.label() == 42 && mi.length() == 7 && mi.extra_caps() == 3
}

fn test_mi_max() -> bool {
    // label is u32 but stored in 52 bits; length is 7 bits (max 127); extra_caps is 2 bits (max 3)
    let mi = MessageInfo::new(0xFFFFF, 120, 3);
    mi.label() == 0xFFFFF && mi.length() == 120 && mi.extra_caps() == 3
}

fn test_mi_from_word_zero() -> bool {
    let mi = MessageInfo::from_word(0);
    mi.word() == 0 && mi.label() == 0
}

fn test_mi_from_word_manual() -> bool {
    // Manually encode: label=10 << 12 | extra_caps=2 << 7 | length=5
    let word: usize = (10 << 12) | (2 << 7) | 5;
    let mi = MessageInfo::from_word(word);
    mi.label() == 10 && mi.length() == 5 && mi.extra_caps() == 2 && mi.word() == word
}

fn test_mi_builder() -> bool {
    let a = MessageInfo::new(100, 5, 2);
    let b = MessageInfoBuilder::new().label(100).length(5).extra_caps(2).build();
    a == b && a.word() == b.word()
}

fn test_mi_builder_empty() -> bool {
    let mi = MessageInfoBuilder::new().build();
    mi.word() == 0
}

fn test_mi_size() -> bool {
    size_of::<MessageInfo>() == size_of::<usize>()
}

// ===========================================================================
// B. CNodeCapData tests
// ===========================================================================

fn test_cn_roundtrip() -> bool {
    let cd = CNodeCapData::new(8, 0x123, 12);
    cd.guard_size() == 8 && cd.radix() == 12 && cd.guard() == 0x123
}

fn test_cn_zero() -> bool {
    let cd = CNodeCapData::new(0, 0, 0);
    cd.word() == 0 && cd.guard_size() == 0 && cd.radix() == 0 && cd.guard() == 0
}

fn test_cn_max() -> bool {
    // guard_size is 6 bits (max 63), radix is 6 bits (max 63)
    let cd = CNodeCapData::new(63, 0, 63);
    cd.guard_size() == 63 && cd.radix() == 63
}

fn test_cn_large_guard() -> bool {
    // guard is stored in bits 12..64, so up to 52 bits on 64-bit
    let cd = CNodeCapData::new(0, 0xFFFFF, 0);
    cd.guard() == 0xFFFFF && cd.guard_size() == 0 && cd.radix() == 0
}

fn test_cn_from_word() -> bool {
    let cd = CNodeCapData::new(10, 0xABC, 20);
    let cd2 = CNodeCapData::from_word(cd.word());
    cd2 == cd
}

// ===========================================================================
// C. CapRights tests
// ===========================================================================

fn test_cr_constants() -> bool {
    CapRights::ALL.bits() == 0b1111
        && CapRights::READ_ONLY.bits() == 0b0010
        && CapRights::READ_WRITE.bits() == 0b0011
}

fn test_cr_default() -> bool {
    CapRights::default().bits() == 0
}

fn test_cr_from_bits() -> bool {
    let cr = CapRights::from_bits(0b1010);
    cr.bits() == 0b1010
}

fn test_cr_custom() -> bool {
    // write + grant = 0b0101
    let cr = CapRights::from_bits(0b0101);
    cr.bits() == 0b0101 && cr != CapRights::ALL && cr != CapRights::READ_WRITE
}

// ===========================================================================
// D. VmAttributes tests
// ===========================================================================

fn test_vm_default() -> bool {
    VmAttributes::DEFAULT.bits() == 0
}

fn test_vm_pat() -> bool {
    let va = VmAttributes::DEFAULT.with_pat(5);
    va.bits() == 5
}

fn test_vm_wt() -> bool {
    let va = VmAttributes::DEFAULT.with_write_through();
    va.bits() == (1 << 3)
}

fn test_vm_cd() -> bool {
    let va = VmAttributes::DEFAULT.with_cache_disabled();
    va.bits() == (1 << 4)
}

fn test_vm_combined() -> bool {
    let va = VmAttributes::DEFAULT.with_write_through().with_cache_disabled();
    va.bits() == (1 << 3) | (1 << 4)
}

// ===========================================================================
// E. ObjectType tests
// ===========================================================================

fn test_ot_all() -> bool {
    ObjectType::Frame4K.size_bits() == 12
        && ObjectType::LargePage.size_bits() == 21
        && ObjectType::PageTable.size_bits() == 12
        && ObjectType::PageDirectory.size_bits() == 12
        && ObjectType::PDPT.size_bits() == 12
        && ObjectType::PML4.size_bits() == 12
        && ObjectType::HugePage.size_bits() == 30
        && ObjectType::IOPageTable.size_bits() == 12
        && ObjectType::TCB.size_bits() == 11
        && ObjectType::Endpoint.size_bits() == 4
        && ObjectType::Notification.size_bits() == 5
}

fn test_ot_variable() -> bool {
    ObjectType::CNode.size_bits() == 0 && ObjectType::Untyped.size_bits() == 0
}

// ===========================================================================
// F. UserContext tests
// ===========================================================================

fn test_uc_default() -> bool {
    let ctx = UserContext::default();
    ctx.ip() == 0 && ctx.sp() == 0 && ctx.rflags == 0x202
        && ctx.rax == 0 && ctx.rbx == 0 && ctx.rcx == 0 && ctx.rdx == 0
        && ctx.rsi == 0 && ctx.rdi == 0 && ctx.rbp == 0
        && ctx.r8 == 0 && ctx.r9 == 0 && ctx.r10 == 0 && ctx.r11 == 0
        && ctx.r12 == 0 && ctx.r13 == 0 && ctx.r14 == 0 && ctx.r15 == 0
        && ctx.fs_base == 0 && ctx.gs_base == 0
}

fn test_uc_ip_sp() -> bool {
    let mut ctx = UserContext::default();
    ctx.set_ip(0xDEAD);
    ctx.set_sp(0xBEEF);
    ctx.ip() == 0xDEAD && ctx.sp() == 0xBEEF && ctx.rip == 0xDEAD && ctx.rsp == 0xBEEF
}

fn test_uc_gpr() -> bool {
    let mut ctx = UserContext::default();
    // gpr mapping: 0=rdi, 1=rsi, 2=rdx, 3=r10, 4=r8, 5=r9
    ctx.set_gpr(0, 100);
    ctx.set_gpr(1, 200);
    ctx.set_gpr(2, 300);
    ctx.set_gpr(3, 400);
    ctx.set_gpr(4, 500);
    ctx.set_gpr(5, 600);
    ctx.gpr(0) == 100 && ctx.gpr(1) == 200 && ctx.gpr(2) == 300
        && ctx.gpr(3) == 400 && ctx.gpr(4) == 500 && ctx.gpr(5) == 600
        && ctx.rdi == 100 && ctx.rsi == 200 && ctx.rdx == 300
        && ctx.r10 == 400 && ctx.r8 == 500 && ctx.r9 == 600
        && ctx.gpr(99) == 0  // out of range returns 0
}

fn test_uc_bytes() -> bool {
    let ctx = UserContext::default();
    ctx.as_bytes().len() == 160
}

fn test_uc_ipc_roundtrip() -> bool {
    let mut ctx = UserContext::default();
    ctx.set_ip(0x1234);
    ctx.set_sp(0x5678);
    ctx.rax = 0xAAAA;
    ctx.rbx = 0xBBBB;

    let mut buf = IpcBuffer::new();
    buf.write_user_context(&ctx);
    let ctx2 = buf.read_user_context();
    ctx2.ip() == 0x1234 && ctx2.sp() == 0x5678 && ctx2.rax == 0xAAAA && ctx2.rbx == 0xBBBB
}

// ===========================================================================
// G. FaultType tests
// ===========================================================================

fn test_ft_all() -> bool {
    FaultType::from_tag(0) == FaultType::NullFault
        && FaultType::from_tag(1) == FaultType::CapFault
        && FaultType::from_tag(2) == FaultType::UnknownSyscall
        && FaultType::from_tag(3) == FaultType::UserException
        && FaultType::from_tag(4) == FaultType::VmFault
        && FaultType::from_tag(5) == FaultType::Timeout
}

fn test_ft_unknown() -> bool {
    FaultType::from_tag(99) == FaultType::NullFault
        && FaultType::from_tag(usize::MAX) == FaultType::NullFault
}

// ===========================================================================
// H. Error tests
// ===========================================================================

fn test_err_all() -> bool {
    Error::from_word(0) == Error::Success
        && Error::from_word(1) == Error::InvalidArgument
        && Error::from_word(2) == Error::InvalidCapability
        && Error::from_word(3) == Error::IllegalOperation
        && Error::from_word(4) == Error::RangeError
        && Error::from_word(5) == Error::AlignmentError
        && Error::from_word(6) == Error::FailedLookup
        && Error::from_word(7) == Error::TruncatedMessage
        && Error::from_word(8) == Error::DeleteFirst
        && Error::from_word(9) == Error::RevokeFirst
        && Error::from_word(10) == Error::NotEnoughMemory
}

fn test_err_unknown() -> bool {
    // Unknown error codes map to InvalidArgument
    Error::from_word(255) == Error::InvalidArgument
        && Error::from_word(100) == Error::InvalidArgument
        && Error::from_word(usize::MAX) == Error::InvalidArgument
}

fn test_err_ok_err() -> bool {
    Error::Success.is_ok() && !Error::Success.is_err()
        && Error::InvalidArgument.is_err() && !Error::InvalidArgument.is_ok()
        && Error::NotEnoughMemory.is_err()
}

fn test_err_display() -> bool {
    // Display should produce non-empty strings
    let s = Error::Success.to_string();
    !s.is_empty() && s == "success"
}

fn test_err_check() -> bool {
    error::check(0).is_ok()
        && error::check(5).is_err()
        && error::check(5).unwrap_err() == Error::AlignmentError
}

// ===========================================================================
// I. IPC Buffer tests
// ===========================================================================

fn test_ipc_tag() -> bool {
    let buf = IpcBuffer::new();
    buf.tag == 0
}

fn test_ipc_mr() -> bool {
    let mut buf = IpcBuffer::new();
    buf.write_mr(0, 0xDEAD_BEEF);
    buf.write_mr(119, 0xCAFE);
    buf.read_mr(0) == 0xDEAD_BEEF && buf.read_mr(119) == 0xCAFE
}

fn test_ipc_mr_bounds() -> bool {
    let mut buf = IpcBuffer::new();
    // Write beyond bounds should be silently ignored
    buf.write_mr(120, 0xFFFF);
    buf.write_mr(1000, 0xFFFF);
    // Read beyond bounds should return 0
    buf.read_mr(120) == 0 && buf.read_mr(1000) == 0 && buf.read_mr(usize::MAX) == 0
}

fn test_ipc_recv_slot() -> bool {
    let mut buf = IpcBuffer::new();
    buf.set_receive_slot(42, 100, 64);
    buf.receive_cnode == 42 && buf.receive_index == 100 && buf.receive_depth == 64
}

fn test_ipc_user_ctx() -> bool {
    let mut buf = IpcBuffer::new();
    let mut ctx = UserContext::default();
    ctx.set_ip(0xAAAA);
    ctx.set_sp(0xBBBB);
    ctx.rflags = 0x202;
    buf.write_user_context(&ctx);
    let ctx2 = buf.read_user_context();
    ctx2.ip() == 0xAAAA && ctx2.sp() == 0xBBBB && ctx2.rflags == 0x202
}

fn test_ipc_words() -> bool {
    let mut buf = IpcBuffer::new();
    let data = [10, 20, 30, 40, 50];
    buf.write_words(&data);
    let slice = buf.read_words(5);
    slice[0] == 10 && slice[1] == 20 && slice[2] == 30 && slice[3] == 40 && slice[4] == 50
}

fn test_ipc_size() -> bool {
    size_of::<IpcBuffer>() == IPC_BUFFER_SIZE
}

// ===========================================================================
// J. Constants tests
// ===========================================================================

fn test_const_syscall_vals() -> bool {
    SYS_CALL as isize == -1
        && SYS_REPLY_RECV as isize == -2
        && SYS_SEND as isize == -3
        && SYS_NBSEND as isize == -4
        && SYS_RECV as isize == -5
        && SYS_REPLY as isize == -6
        && SYS_YIELD as isize == -7
        && SYS_NBRECV as isize == -8
        && SYS_DEBUG_PUT_CHAR as isize == -9
        && SYS_DEBUG_DUMP_SCHEDULER as isize == -10
        && SYS_DEBUG_HALT as isize == -11
        && SYS_SIGNAL as isize == -12
        && SYS_SET_TLS_BASE as isize == -29
}

fn test_const_syscall_distinct() -> bool {
    let vals = [
        SYS_CALL, SYS_REPLY_RECV, SYS_SEND, SYS_NBSEND, SYS_RECV,
        SYS_REPLY, SYS_YIELD, SYS_NBRECV, SYS_DEBUG_PUT_CHAR,
        SYS_DEBUG_DUMP_SCHEDULER, SYS_DEBUG_HALT, SYS_SIGNAL, SYS_SET_TLS_BASE,
    ];
    for i in 0..vals.len() {
        for j in (i + 1)..vals.len() {
            if vals[i] == vals[j] {
                return false;
            }
        }
    }
    true
}

fn test_const_init_slots() -> bool {
    init_slots::TCB == 1
        && init_slots::CNODE == 2
        && init_slots::VSPACE == 3
        && init_slots::IRQ_CONTROL == 4
        && init_slots::ASID_CONTROL == 5
        && init_slots::ASID_POOL == 6
        && init_slots::IO_PORT_CONTROL == 7
        && init_slots::NUM_INITIAL_CAPS == 16
        && init_slots::FIRST_FREE > init_slots::NUM_INITIAL_CAPS
}

fn test_const_ipc() -> bool {
    IPC_BUFFER_MSG_REGS == 120 && IPC_BUFFER_SIZE == 4096 && IPC_BUFFER_ALIGN == 4096
}

// ===========================================================================
// K. Runtime syscall tests
// ===========================================================================

fn test_rt_putstring() -> bool {
    seL4_DebugPutString("seL4-sys-test");
    true // no crash = pass
}

fn test_rt_dumpsched() -> bool {
    seL4_DebugDumpScheduler();
    true
}

fn test_rt_yield() -> bool {
    seL4_Yield();
    true
}

// ===========================================================================
// L. IPC Buffer TLS tests
// ===========================================================================

fn test_tls_ipc() -> bool {
    with_ipc_buffer(|buf| {
        buf.write_mr(0, 0x1234_5678);
        buf.read_mr(0) == 0x1234_5678
    })
}
