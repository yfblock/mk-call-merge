//! seL4 Root Task — entry point and test runner for x86_64.
//!
//! This is the first userspace task started by the seL4 kernel. It:
//!
//! 1. Sets up the runtime environment (stack, IPC buffer, slot manager).
//! 2. Runs a series of self-tests, outputting results via the kernel debug
//!    serial port (`seL4_DebugPutChar`).
//!
//! # Entry Point
//!
//! The linker entry point is `_start` (defined in the inline assembly below).
//! It sets up the stack and calls `sel4_runtime_rust_entry`, which is the
//! actual Rust entry function.

#![no_std]
#![no_main]
#![allow(non_snake_case)]
#![feature(alloc_error_handler)]
#![feature(thread_local)]
#![allow(internal_features)]

extern crate alloc;

use core::arch::global_asm;
use core::panic::PanicInfo;
use sel4_sys::*;

// ---------------------------------------------------------------------------
// Global allocator — simple bump allocator
// ---------------------------------------------------------------------------

/// Heap memory region. We use a simple bump allocator for early development.
const HEAP_SIZE: usize = 0x10_0000; // 1 MiB
static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];
static mut HEAP_OFFSET: usize = 0;

/// Minimal allocator error handler.
#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! {
    panic!(
        "memory allocation failed: size={}, align={}",
        layout.size(),
        layout.align()
    );
}

/// Bump allocator for global heap allocations.
struct BumpAllocator;

unsafe impl core::alloc::GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        // SAFETY: single-threaded root task, no concurrent access.
        unsafe {
            let offset = HEAP_OFFSET;
            let align = layout.align();
            let aligned = (offset + align - 1) & !(align - 1);
            let new_offset = aligned + layout.size();
            if new_offset > HEAP_SIZE {
                core::ptr::null_mut()
            } else {
                HEAP_OFFSET = new_offset;
                // Use raw pointer arithmetic to avoid mutable reference to
                // mutable static (denied in Rust 2024).
                let base: *mut u8 = core::ptr::addr_of_mut!(HEAP).cast::<u8>();
                base.add(aligned)
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: core::alloc::Layout) {
        // Bump allocator does not support deallocation.
        // This is acceptable for a root task that runs until shutdown.
    }
}

#[global_allocator]
static GLOBAL_ALLOC: BumpAllocator = BumpAllocator;

// ---------------------------------------------------------------------------
// Stack configuration
// ---------------------------------------------------------------------------

/// Stack top address for the root task.
const STACK_TOP_ADDR: usize = 0x1_0000_0000;

// ---------------------------------------------------------------------------
// Entry point assembly
// ---------------------------------------------------------------------------

global_asm! {
    r#"
        .extern sel4_runtime_rust_entry
        .extern __sel4_runtime_common__stack_bottom

        .global _start

        .section .text

    _start:
        mov rsp, __sel4_runtime_common__stack_bottom
        mov rbp, rsp
        sub rsp, 0x8          // Stack must be 16-byte aligned before call
        push rbp
        call sel4_runtime_rust_entry

    1:  jmp 1b
    "#
}

/// Stack bottom symbol, loaded by the assembly trampoline to set the
/// initial stack pointer. The value stored here is the *address* of the
/// stack top, not the stack bottom (historical naming from rust-sel4).
#[unsafe(export_name = "__sel4_runtime_common__stack_bottom")]
static STACK_BOTTOM: usize = STACK_TOP_ADDR;

// ---------------------------------------------------------------------------
// Rust entry point
// ---------------------------------------------------------------------------

/// The main Rust entry point, called from the assembly trampoline.
///
/// This function initializes the runtime and executes the main application
/// logic.
#[unsafe(export_name = "sel4_runtime_rust_entry")]
unsafe extern "C" fn rust_entry() -> ! {
    // Initialize the IPC buffer (placed after the end of the ELF image).
    init_ipc_buffer();

    // Run the main application.
    main();

    // If main returns, spin forever.
    loop {
        core::hint::spin_loop();
    }
}

/// Initialize the IPC buffer for the root task.
///
/// The IPC buffer is placed right after the end of the program's data
/// segment, page-aligned.
fn init_ipc_buffer() {
    unsafe {
        // The `_end` symbol is defined by the linker script, marking the
        // end of the program's data/bss segments.
        unsafe extern "C" {
            static _end: u8;
        }
        let end_addr = core::ptr::addr_of!(_end) as usize;
        let ipc_buf_addr = end_addr.next_multiple_of(IPC_BUFFER_ALIGN);
        let ipc_buf = &mut *(ipc_buf_addr as *mut IpcBuffer);
        // Zero-initialize and configure.
        *ipc_buf = IpcBuffer::new();
        ipc_buf.set_receive_slot(init_slots::CNODE, 0, 64);
        set_ipc_buffer(ipc_buf);
    }
}

// ---------------------------------------------------------------------------
// Panic handler
// ---------------------------------------------------------------------------

/// Panic handler — called when a Rust panic occurs.
///
/// Outputs the panic message to the kernel debug serial and halts.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    seL4_DebugPutString("\n\n=== PANIC ===\n");
    if let Some(location) = info.location() {
        seL4_DebugPutString("Location: ");
        seL4_DebugPutString(location.file());
        seL4_DebugPutChar(b':');
        seL4_DebugPutU64(location.line() as u64);
        seL4_DebugPutChar(b'\n');
    }
    if info.message().as_str().is_some() {
        seL4_DebugPutString("Message: (see panic info above)\n");
    }
    seL4_DebugPutString("System halted.\n");

    // Halt: spin forever.
    loop {
        core::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// Core constants
// ---------------------------------------------------------------------------

/// Page size for memory mapping.
const PAGE_SIZE: usize = 4096;

/// Well-known capability slot addresses (matching the kernel's initial
/// CSpace layout for the root task).
#[allow(dead_code)]
mod slots {
    pub const CNODE: usize = 1;
    pub const VSPACE: usize = 2;
    pub const ASID_POOL: usize = 3;
    pub const IRQ_CONTROL: usize = 4;
    pub const ASID_CONTROL: usize = 5;
    pub const TCB: usize = 6;
    pub const FIRST_FREE: usize = 7;
}

// ---------------------------------------------------------------------------
// Slot manager — simple linear allocator for capability slots
// ---------------------------------------------------------------------------

/// A simple capability slot allocator.
///
/// Allocates slots sequentially from a free range. Recycling (freeing) is
/// not yet supported — slots are consumed monotonically.
struct SlotManager {
    next: usize,
    end: usize,
}

impl SlotManager {
    const fn new(start: usize, end: usize) -> Self {
        Self { next: start, end }
    }

    fn alloc(&mut self) -> Option<usize> {
        if self.next >= self.end {
            return None;
        }
        let slot = self.next;
        self.next += 1;
        Some(slot)
    }

    fn available(&self) -> usize {
        self.end - self.next
    }
}

/// Global slot manager. Protected by a simple spin mutex.
///
/// Since the root task is single-threaded, a `RefCell` would suffice, but
/// we use a lock-free spin lock to keep the interface simple and avoid
/// `UnsafeCell` boilerplate.
static SLOT_MANAGER: SimpleMutex<SlotManager> =
    SimpleMutex::new(SlotManager::new(slots::FIRST_FREE, 0x1000));

// ---------------------------------------------------------------------------
// Simple mutex (busy-wait, for single-threaded use)
// ---------------------------------------------------------------------------

/// A minimal lock for use in single-threaded environments.
///
/// In the root task, there is no preemption, so a simple `Cell<bool>` is
/// sufficient. The lock methods use `spin_loop` for safety but will never
/// actually spin in practice.
struct SimpleMutex<T> {
    locked: core::cell::Cell<bool>,
    data: core::cell::UnsafeCell<T>,
}

// SAFETY: Single-threaded root task.
unsafe impl<T> Sync for SimpleMutex<T> {}

impl<T> SimpleMutex<T> {
    const fn new(data: T) -> Self {
        Self {
            locked: core::cell::Cell::new(false),
            data: core::cell::UnsafeCell::new(data),
        }
    }

    fn lock(&self) -> SimpleMutexGuard<'_, T> {
        while self.locked.replace(true) {
            core::hint::spin_loop();
        }
        SimpleMutexGuard { mutex: self }
    }
}

struct SimpleMutexGuard<'a, T> {
    mutex: &'a SimpleMutex<T>,
}

impl<T> core::ops::Deref for SimpleMutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: We hold the lock.
        unsafe { &*self.mutex.data.get() }
    }
}

impl<T> core::ops::DerefMut for SimpleMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: We hold the lock.
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<T> Drop for SimpleMutexGuard<'_, T> {
    fn drop(&mut self) {
        self.mutex.locked.set(false);
    }
}

// ---------------------------------------------------------------------------
// Simple serial output helpers
// ---------------------------------------------------------------------------

/// Output a u64 as a decimal number to the kernel debug serial.
fn seL4_DebugPutU64(val: u64) {
    if val == 0 {
        seL4_DebugPutChar(b'0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20;
    let mut v = val;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    for b in &buf[i..] {
        seL4_DebugPutChar(*b);
    }
}

/// Output a u64 as a hex string to the kernel debug serial.
fn seL4_DebugPutHex(val: u64) {
    seL4_DebugPutString("0x");
    for shift in (0..16).rev() {
        let nibble = ((val >> (shift * 4)) & 0xf) as u8;
        let c = if nibble < 10 {
            b'0' + nibble
        } else {
            b'a' + (nibble - 10)
        };
        seL4_DebugPutChar(c);
    }
}

/// Output a boolean value to the kernel debug serial.
#[allow(dead_code)]
fn seL4_DebugPutBool(val: bool) {
    if val {
        seL4_DebugPutString("true");
    } else {
        seL4_DebugPutString("false");
    }
}

// ---------------------------------------------------------------------------
// Test framework
// ---------------------------------------------------------------------------

/// A single test case.
struct TestCase {
    name: &'static str,
    func: fn() -> bool,
}

/// Global test registry.
static TESTS: [TestCase; 16] = [
    TestCase {
        name: "seL4_DebugPutChar",
        func: test_debug_putchar,
    },
    TestCase {
        name: "seL4_Yield",
        func: test_yield,
    },
    TestCase {
        name: "IPC buffer read/write",
        func: test_ipc_buffer,
    },
    TestCase {
        name: "Slot allocation",
        func: test_slot_allocation,
    },
    TestCase {
        name: "Object type constants",
        func: test_object_types,
    },
    TestCase {
        name: "MessageInfo encoding",
        func: test_message_info,
    },
    TestCase {
        name: "CNodeCapData encoding",
        func: test_cnode_cap_data,
    },
    TestCase {
        name: "CapRights constants",
        func: test_cap_rights,
    },
    TestCase {
        name: "UserContext layout",
        func: test_user_context,
    },
    TestCase {
        name: "Fault type encoding",
        func: test_fault_type,
    },
    TestCase {
        name: "IPC buffer layout size",
        func: test_ipc_buffer_size,
    },
    TestCase {
        name: "Untyped region parsing",
        func: test_untyped_desc,
    },
    TestCase {
        name: "Error codes",
        func: test_error_codes,
    },
    TestCase {
        name: "Slot init_slots",
        func: test_init_slots,
    },
    TestCase {
        name: "Syscall numbers",
        func: test_syscall_numbers,
    },
    TestCase {
        name: "VMAttributes defaults",
        func: test_vm_attributes,
    },
];

// ---------------------------------------------------------------------------
// Test implementations
// ---------------------------------------------------------------------------

/// Test: DebugPutChar produces output.
fn test_debug_putchar() -> bool {
    seL4_DebugPutChar(b'X');
    true
}

/// Test: Yield does not crash.
fn test_yield() -> bool {
    seL4_Yield();
    true
}

/// Test: IPC buffer read/write works.
fn test_ipc_buffer() -> bool {
    with_ipc_buffer(|ipc_buf| {
        ipc_buf.write_mr(0, 0xDEAD_BEEF);
        let val = ipc_buf.read_mr(0);
        val == 0xDEAD_BEEF
    })
}

/// Test: Slot allocation works.
fn test_slot_allocation() -> bool {
    let mut sm = SLOT_MANAGER.lock();
    let initial = sm.available();
    let s1 = sm.alloc();
    initial > 0 && s1.is_some()
}

/// Test: Object type constants are valid.
fn test_object_types() -> bool {
    use sel4_sys::ObjectType;
    ObjectType::Frame4K.size_bits() == 12
        && ObjectType::LargePage.size_bits() == 21
        && ObjectType::PageTable.size_bits() == 12
        && ObjectType::TCB.size_bits() == 11
}

/// Test: MessageInfo encoding and decoding.
fn test_message_info() -> bool {
    let mi = MessageInfo::new(42, 7, 3);
    mi.label() == 42 && mi.length() == 7 && mi.extra_caps() == 3
}

/// Test: CNodeCapData encoding.
fn test_cnode_cap_data() -> bool {
    let cd = CNodeCapData::new(8, 0x123, 12);
    cd.guard_size() == 8 && cd.radix() == 12 && cd.guard() == 0x123
}

/// Test: CapRights constants.
fn test_cap_rights() -> bool {
    let rw = CapRights::READ_WRITE;
    let ro = CapRights::READ_ONLY;
    let all = CapRights::ALL;
    rw.bits() == 0b0011 && ro.bits() == 0b0010 && all.bits() == 0b1111
}

/// Test: UserContext layout.
fn test_user_context() -> bool {
    let ctx = UserContext::default();
    ctx.ip() == 0 && ctx.sp() == 0 && ctx.rflags == 0x202
}

/// Test: Fault type encoding.
fn test_fault_type() -> bool {
    use sel4_sys::FaultType;
    let ft = FaultType::from_tag(3);
    ft == FaultType::UserException
}

/// Test: IPC buffer has correct size.
fn test_ipc_buffer_size() -> bool {
    core::mem::size_of::<IpcBuffer>() == IPC_BUFFER_SIZE
}

/// Test: UntypedDesc layout and default.
fn test_untyped_desc() -> bool {
    let desc = sel4_sys::UntypedDesc {
        paddr: 0x100000,
        size_bits: 20,
        is_device: 0,
        padding: [0; 6],
    };
    desc.paddr == 0x100000 && desc.size_bits == 20 && desc.is_device == 0
}

/// Test: Error code to/from word.
fn test_error_codes() -> bool {
    use sel4_sys::Error;
    let ok = Error::from_word(0);
    let err = Error::from_word(2);
    ok.is_ok() && err.is_err() && ok == Error::Success && err == Error::InvalidCapability
}

/// Test: Init slots are valid.
fn test_init_slots() -> bool {
    init_slots::CNODE > 0 && init_slots::VSPACE > 0 && init_slots::TCB > 0
}

/// Test: Syscall number constants are distinct.
fn test_syscall_numbers() -> bool {
    SYS_SEND != SYS_CALL
        && SYS_CALL != SYS_RECV
        && SYS_RECV != SYS_REPLY
        && SYS_YIELD != SYS_DEBUG_PUT_CHAR
}

/// Test: VmAttributes defaults.
fn test_vm_attributes() -> bool {
    use sel4_sys::VmAttributes;
    let def = VmAttributes::DEFAULT;
    def.bits() == 0
}

// ---------------------------------------------------------------------------
// Main application
// ---------------------------------------------------------------------------

/// Main application entry point.
///
/// Initializes the system and runs all tests.
fn main() {
    seL4_DebugPutString("\n========================================\n");
    seL4_DebugPutString("  rel4-linux-kit -- seL4 x86_64 Tests\n");
    seL4_DebugPutString("========================================\n\n");

    // Run all tests.
    let mut passed = 0usize;
    let mut failed = 0usize;

    for test in &TESTS {
        seL4_DebugPutString("  TEST: ");
        seL4_DebugPutString(test.name);
        seL4_DebugPutString(" ... ");

        let result = (test.func)();
        if result {
            seL4_DebugPutString("PASSED\n");
            passed += 1;
        } else {
            seL4_DebugPutString("FAILED\n");
            failed += 1;
        }
    }

    // Print summary.
    seL4_DebugPutString("\n----------------------------------------\n");
    seL4_DebugPutString("  Results: ");
    seL4_DebugPutU64(passed as u64);
    seL4_DebugPutString(" passed, ");
    seL4_DebugPutU64(failed as u64);
    seL4_DebugPutString(" failed");
    if failed > 0 {
        seL4_DebugPutString(" (SOME TESTS FAILED)\n");
    } else {
        seL4_DebugPutString(" (all tests passed)\n");
    }
    seL4_DebugPutString("----------------------------------------\n\n");

    // Print system information.
    print_system_info();

    seL4_DebugPutString("\nRoot task completed successfully.\n");
    seL4_DebugPutString("System will yield indefinitely.\n\n");

    // Yield forever.
    loop {
        seL4_Yield();
    }
}

/// Print system information.
fn print_system_info() {
    seL4_DebugPutString("=== System Information ===\n");
    seL4_DebugPutString("  Architecture: x86_64\n");

    let sm = SLOT_MANAGER.lock();
    seL4_DebugPutString("  Free capability slots: ");
    seL4_DebugPutU64(sm.available() as u64);
    seL4_DebugPutString("\n");

    seL4_DebugPutString("  Page size: ");
    seL4_DebugPutU64(PAGE_SIZE as u64);
    seL4_DebugPutString(" bytes\n");

    seL4_DebugPutString("  Heap size: ");
    seL4_DebugPutU64(HEAP_SIZE as u64);
    seL4_DebugPutString(" bytes\n");

    seL4_DebugPutString("  Stack base: ");
    seL4_DebugPutHex(STACK_TOP_ADDR as u64);
    seL4_DebugPutString("\n");

    seL4_DebugPutString("  IPC buffer size: ");
    seL4_DebugPutU64(IPC_BUFFER_SIZE as u64);
    seL4_DebugPutString(" bytes\n");

    seL4_DebugPutString("===========================\n");
}
