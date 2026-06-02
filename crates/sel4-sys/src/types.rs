//! Kernel ABI types for seL4 on x86_64.
//!
//! These types have `#[repr(C)]` layout to match the kernel's binary interface.
//! They are used to interpret the data exchanged via the IPC buffer and to
//! construct invocation arguments.
//!
//! Note: `#[repr(C)]` is necessary here for ABI compatibility with the seL4
//! kernel — it is NOT used to call C functions. All system calls are performed
//! via inline assembly in the [`syscalls`](crate::syscalls) module.

use core::fmt;

// ---------------------------------------------------------------------------
// MessageInfo — describes the contents of an IPC message
// ---------------------------------------------------------------------------

/// Wrapper around the raw `seL4_MessageInfo` word.
///
/// The layout (from LSB to MSB):
/// - Bits 0..7:   Length (number of message registers used)
/// - Bits 7..12:  Number of extra capabilities transferred
/// - Bits 12..32: Invocation label (for TCB/CNode/etc. operations)
/// - Bits 32..64: (reserved / unused on 64-bit)
#[derive(Clone, Copy, PartialEq, Eq, Default)]
#[repr(transparent)]
pub struct MessageInfo {
    word: usize,
}

impl MessageInfo {
    /// Create a new `MessageInfo` from a raw word.
    #[inline]
    pub const fn from_word(word: usize) -> Self {
        Self { word }
    }

    /// Get the raw word representation.
    #[inline]
    pub const fn word(self) -> usize {
        self.word
    }

    /// Create a `MessageInfo` with the given label, length, and cap count.
    #[inline]
    pub const fn new(label: u32, length: u8, extra_caps: u8) -> Self {
        Self {
            word: ((label as usize) << 12) | ((extra_caps as usize) << 7) | (length as usize),
        }
    }

    /// Get the label field (bits 12..).
    #[inline]
    pub fn label(self) -> u32 {
        (self.word >> 12) as u32
    }

    /// Get the number of message registers (bits 0..7).
    #[inline]
    pub fn length(self) -> u8 {
        (self.word & 0x7f) as u8
    }

    /// Get the number of extra capabilities transferred (bits 7..12).
    #[inline]
    pub fn extra_caps(self) -> u8 {
        ((self.word >> 7) & 0x1f) as u8
    }
}

impl fmt::Debug for MessageInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MessageInfo")
            .field("label", &self.label())
            .field("length", &self.length())
            .field("extra_caps", &self.extra_caps())
            .finish()
    }
}

/// Builder for constructing `MessageInfo` values.
pub struct MessageInfoBuilder {
    label: u32,
    length: u8,
    extra_caps: u8,
}

impl MessageInfoBuilder {
    pub const fn new() -> Self {
        Self {
            label: 0,
            length: 0,
            extra_caps: 0,
        }
    }

    pub const fn label(mut self, label: u32) -> Self {
        self.label = label;
        self
    }

    pub const fn length(mut self, length: u8) -> Self {
        self.length = length;
        self
    }

    pub const fn extra_caps(mut self, extra_caps: u8) -> Self {
        self.extra_caps = extra_caps;
        self
    }

    pub const fn build(self) -> MessageInfo {
        MessageInfo::new(self.label, self.length, self.extra_caps)
    }
}

// ---------------------------------------------------------------------------
// CNodeCapData — depth and guard for CNode capabilities
// ---------------------------------------------------------------------------

/// Encodes the depth and guard fields used when configuring a CNode capability.
///
/// Layout:
/// - Bits 0..6:   Guard size (number of guard bits)
/// - Bits 6..12:  CNode radix (number of bits resolved, i.e. depth)
/// - Bits 12..64: Guard value
#[derive(Clone, Copy, PartialEq, Eq, Default)]
#[repr(transparent)]
pub struct CNodeCapData {
    word: usize,
}

impl CNodeCapData {
    /// Create from a raw word.
    #[inline]
    pub const fn from_word(word: usize) -> Self {
        Self { word }
    }

    /// Get the raw word.
    #[inline]
    pub const fn word(self) -> usize {
        self.word
    }

    /// Create new CNodeCapData with the given guard size, guard value, and
    /// radix (depth).
    #[inline]
    pub const fn new(guard_size: u8, guard: usize, radix: u8) -> Self {
        Self {
            word: (guard << 12) | ((radix as usize) << 6) | (guard_size as usize),
        }
    }

    /// Get the guard size.
    #[inline]
    pub fn guard_size(self) -> u8 {
        (self.word & 0x3f) as u8
    }

    /// Get the radix (depth).
    #[inline]
    pub fn radix(self) -> u8 {
        ((self.word >> 6) & 0x3f) as u8
    }

    /// Get the guard value.
    #[inline]
    pub fn guard(self) -> usize {
        self.word >> 12
    }
}

impl fmt::Debug for CNodeCapData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CNodeCapData")
            .field("guard_size", &self.guard_size())
            .field("radix", &self.radix())
            .field("guard", &self.guard())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// CapRights — access rights for capabilities
// ---------------------------------------------------------------------------

/// Access rights mask for a capability (read, write, grant, etc.).
///
/// Layout:
/// - Bit 0: Write
/// - Bit 1: Read
/// - Bit 2: Grant
/// - Bit 3: GrantReply
#[derive(Clone, Copy, PartialEq, Eq, Default)]
#[repr(transparent)]
pub struct CapRights {
    bits: usize,
}

impl CapRights {
    /// Full rights (read + write + grant + grant-reply).
    pub const ALL: Self = Self {
        bits: 0b1111,
    };

    /// Read-only rights.
    pub const READ_ONLY: Self = Self {
        bits: 0b0010,
    };

    /// Read + Write rights.
    pub const READ_WRITE: Self = Self {
        bits: 0b0011,
    };

    /// Create from raw bits.
    #[inline]
    pub const fn from_bits(bits: usize) -> Self {
        Self { bits }
    }

    /// Get the raw bits.
    #[inline]
    pub const fn bits(self) -> usize {
        self.bits
    }
}

// ---------------------------------------------------------------------------
// VmAttributes — x86_64 page mapping attributes
// ---------------------------------------------------------------------------

/// Virtual memory attributes for page mappings on x86_64.
///
/// These correspond to `seL4_X86_VMAttributes`:
/// - Bits 0..2: Page cacheability (PAT / cache disable / write-through)
/// - Bit 3:     Write-through
/// - Bit 4:     Cache disabled
#[derive(Clone, Copy, PartialEq, Eq, Default)]
#[repr(transparent)]
pub struct VmAttributes {
    bits: usize,
}

impl VmAttributes {
    /// Default attributes (write-back caching, no special flags).
    pub const DEFAULT: Self = Self { bits: 0 };

    /// Create from raw bits.
    #[inline]
    pub const fn from_bits(bits: usize) -> Self {
        Self { bits }
    }

    /// Get the raw bits.
    #[inline]
    pub const fn bits(self) -> usize {
        self.bits
    }

    /// Set the PAT (Page Attribute Table) index.
    #[inline]
    pub const fn with_pat(self, pat: u8) -> Self {
        Self {
            bits: (self.bits & !0x7) | ((pat as usize) & 0x7),
        }
    }

    /// Enable write-through caching.
    #[inline]
    pub const fn with_write_through(self) -> Self {
        Self {
            bits: self.bits | (1 << 3),
        }
    }

    /// Disable caching.
    #[inline]
    pub const fn with_cache_disabled(self) -> Self {
        Self {
            bits: self.bits | (1 << 4),
        }
    }
}

// ---------------------------------------------------------------------------
// Object types for Untyped_Retype
// ---------------------------------------------------------------------------

/// Kernel object types for `seL4_Untyped_Retype`.
///
/// Values match the kernel's combined enum (api_object + mode + arch objects).
/// Chain: non-arch(0..4) → mode(5..7) → arch(8..12)
/// CONFIG_HUGE_PAGE=1 adds HugePage, CONFIG_IOMMU=1 adds IOPageTable.
#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectType {
    /// Untyped raw memory (retype source).
    Untyped = 0,
    /// Thread Control Block.
    TCB = 1,
    /// IPC endpoint.
    Endpoint = 2,
    /// Async notification object.
    Notification = 3,
    /// Capability node (CNode). Size is variable (radix passed as userObjSize).
    CNode = 4,
    /// x86_64 PDPT (page directory pointer table).
    PDPT = 5,
    /// x86_64 PML4 (page map level 4, root page table).
    PML4 = 6,
    /// 1 GiB huge page (CONFIG_HUGE_PAGE).
    HugePage = 7,
    /// 4 KiB page frame.
    Frame4K = 8,
    /// 2 MiB large page frame.
    LargePage = 9,
    /// x86 page table.
    PageTable = 10,
    /// x86 page directory.
    PageDirectory = 11,
    /// I/O page table (CONFIG_IOMMU).
    IOPageTable = 12,
}

impl ObjectType {
    /// Get the size bits for this object type.
    ///
    /// Size bits = log2(size in bytes). For example, a 4K frame has size_bits = 12.
    /// For variable-size types (CNode, Untyped), the caller must provide the
    /// appropriate value — this method returns 0 as a placeholder.
    pub fn size_bits(self) -> usize {
        match self {
            ObjectType::Untyped => 0,        // variable
            ObjectType::TCB => 11,           // seL4_TCBBits = 11 (2 KiB)
            ObjectType::Endpoint => 4,       // seL4_EndpointBits = 4 (16 bytes)
            ObjectType::Notification => 5,   // seL4_NotificationBits = 5 (32 bytes, non-MCS)
            ObjectType::CNode => 0,          // variable; size_bits is the radix
            ObjectType::PDPT => 12,          // seL4_PageBits = 12 (4 KiB)
            ObjectType::PML4 => 12,          // seL4_PageBits = 12 (4 KiB)
            ObjectType::HugePage => 30,      // seL4_HugePageBits = 30 (1 GiB)
            ObjectType::Frame4K => 12,       // seL4_PageBits = 12 (4 KiB)
            ObjectType::LargePage => 21,     // seL4_LargePageBits = 21 (2 MiB)
            ObjectType::PageTable => 12,     // seL4_PageTableBits = 12 (4 KiB)
            ObjectType::PageDirectory => 12, // seL4_PageBits = 12 (4 KiB)
            ObjectType::IOPageTable => 12,   // seL4_IOPageTableBits = 12 (4 KiB)
        }
    }
}

// ---------------------------------------------------------------------------
// UserContext — register state for a thread
// ---------------------------------------------------------------------------

/// Register state for an x86_64 thread, corresponding to `seL4_UserContext`.
///
/// The layout for `seL4_UserContext` on x86_64 is:
/// ```c
/// typedef struct seL4_UserContext_ {
///     seL4_Word rip, rsp, rflags, rax, rbx, rcx, rdx,
///               rsi, rdi, rbp, r8, r9, r10, r11, r12,
///               r13, r14, r15, fs_base, gs_base;
/// } seL4_UserContext;
/// ```
///
/// Total: 20 × 8 = 160 bytes on x86_64.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UserContext {
    /// Instruction pointer (RIP)
    pub rip: u64,
    /// Stack pointer (RSP)
    pub rsp: u64,
    /// Flags register (RFLAGS)
    pub rflags: u64,
    /// General-purpose register RAX
    pub rax: u64,
    /// General-purpose register RBX
    pub rbx: u64,
    /// General-purpose register RCX
    pub rcx: u64,
    /// General-purpose register RDX
    pub rdx: u64,
    /// General-purpose register RSI
    pub rsi: u64,
    /// General-purpose register RDI
    pub rdi: u64,
    /// Base pointer (RBP)
    pub rbp: u64,
    /// General-purpose register R8
    pub r8: u64,
    /// General-purpose register R9
    pub r9: u64,
    /// General-purpose register R10
    pub r10: u64,
    /// General-purpose register R11
    pub r11: u64,
    /// General-purpose register R12
    pub r12: u64,
    /// General-purpose register R13
    pub r13: u64,
    /// General-purpose register R14
    pub r14: u64,
    /// General-purpose register R15
    pub r15: u64,
    /// FS base register (TLS)
    pub fs_base: u64,
    /// GS base register
    pub gs_base: u64,
}

impl UserContext {
    /// Create a default (zeroed) user context.
    pub const fn default() -> Self {
        Self {
            rip: 0,
            rsp: 0,
            rflags: 0x202, // Default: interrupt flag set
            rax: 0,
            rbx: 0,
            rcx: 0,
            rdx: 0,
            rsi: 0,
            rdi: 0,
            rbp: 0,
            r8: 0,
            r9: 0,
            r10: 0,
            r11: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            fs_base: 0,
            gs_base: 0,
        }
    }

    /// Get a pointer to the byte representation (for IPC buffer read/write).
    pub fn as_bytes(&self) -> &[u8; 160] {
        unsafe { &*(self as *const Self as *const [u8; 160]) }
    }

    /// Get a mutable pointer to the byte representation.
    pub fn as_bytes_mut(&mut self) -> &mut [u8; 160] {
        unsafe { &mut *(self as *mut Self as *mut [u8; 160]) }
    }

    /// Get the instruction pointer.
    pub fn ip(&self) -> u64 {
        self.rip
    }

    /// Set the instruction pointer.
    pub fn set_ip(&mut self, ip: u64) {
        self.rip = ip;
    }

    /// Get the stack pointer.
    pub fn sp(&self) -> u64 {
        self.rsp
    }

    /// Set the stack pointer.
    pub fn set_sp(&mut self, sp: u64) {
        self.rsp = sp;
    }

    /// Get a general-purpose register by index (for syscall argument access).
    ///
    /// Mapping used by Linux syscall convention:
    /// - arg0 → rdi (index 0)
    /// - arg1 → rsi (index 1)
    /// - arg2 → rdx (index 2)
    /// - arg3 → r10 (index 3)
    /// - arg4 → r8  (index 4)
    /// - arg5 → r9  (index 5)
    /// - return → rax
    pub fn gpr(&self, idx: usize) -> u64 {
        match idx {
            0 => self.rdi,
            1 => self.rsi,
            2 => self.rdx,
            3 => self.r10,
            4 => self.r8,
            5 => self.r9,
            _ => 0,
        }
    }

    /// Set a general-purpose register.
    pub fn set_gpr(&mut self, idx: usize, val: u64) {
        match idx {
            0 => self.rdi = val,
            1 => self.rsi = val,
            2 => self.rdx = val,
            3 => self.r10 = val,
            4 => self.r8 = val,
            5 => self.r9 = val,
            _ => {}
        }
    }
}

impl fmt::Debug for UserContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserContext")
            .field("rip", &format_args!("{:#x}", self.rip))
            .field("rsp", &format_args!("{:#x}", self.rsp))
            .field("rflags", &format_args!("{:#x}", self.rflags))
            .field("rax", &format_args!("{:#x}", self.rax))
            .field("rdi", &format_args!("{:#x}", self.rdi))
            .field("rsi", &format_args!("{:#x}", self.rsi))
            .field("rdx", &format_args!("{:#x}", self.rdx))
            .field("r10", &format_args!("{:#x}", self.r10))
            .field("r8", &format_args!("{:#x}", self.r8))
            .field("r9", &format_args!("{:#x}", self.r9))
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Fault types — delivered via fault endpoint
// ---------------------------------------------------------------------------

/// Fault type delivered by the kernel when a thread faults.
///
/// This is a simplified representation. The actual seL4 fault format depends on
/// the architecture and fault type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultType {
    /// Null fault (no fault).
    NullFault = 0,
    /// Capability fault.
    CapFault = 1,
    /// Unknown syscall fault.
    UnknownSyscall = 2,
    /// User exception (e.g., page fault, illegal instruction).
    UserException = 3,
    /// Virtual memory fault.
    VmFault = 4,
    /// Timeout fault (MCS kernel only).
    Timeout = 5,
}

impl FaultType {
    /// Create from the raw fault tag value from the kernel.
    pub fn from_tag(tag: usize) -> Self {
        match tag {
            0 => FaultType::NullFault,
            1 => FaultType::CapFault,
            2 => FaultType::UnknownSyscall,
            3 => FaultType::UserException,
            4 => FaultType::VmFault,
            5 => FaultType::Timeout,
            _ => FaultType::NullFault, // Unknown, treat as null
        }
    }
}

// ---------------------------------------------------------------------------
// Slot addresses — well-known slots in the initial CSpace
// ---------------------------------------------------------------------------

/// Well-known capability slot addresses in the initial thread's CSpace.
///
/// These constants reference the slots set up by the kernel for the root task.
/// Values match `seL4_CapInit*` from `sel4/bootinfo_types.h`.
pub mod init_slots {
    /// Null capability (always empty).
    pub const NULL: usize = 0;
    /// Root task's TCB.
    pub const TCB: usize = 1;
    /// CSpace root CNode.
    pub const CNODE: usize = 2;
    /// VSpace root (PML4).
    pub const VSPACE: usize = 3;
    /// IRQ Control.
    pub const IRQ_CONTROL: usize = 4;
    /// ASID Control.
    pub const ASID_CONTROL: usize = 5;
    /// ASID Pool.
    pub const ASID_POOL: usize = 6;
    /// IOPort Control (x86 only).
    pub const IO_PORT_CONTROL: usize = 7;
    /// IO Space (IOMMU, x86 only).
    pub const IO_SPACE: usize = 8;
    /// BootInfo frame.
    pub const BOOT_INFO: usize = 9;
    /// IPC buffer frame.
    pub const IPC_BUFFER: usize = 10;
    /// Domain cap.
    pub const DOMAIN: usize = 11;
    /// Scheduling context (MCS only, null otherwise).
    pub const SC: usize = 14;
    /// Number of initial capabilities (untyped slots start here).
    pub const NUM_INITIAL_CAPS: usize = 16;
    /// First free slot for user allocation (after initial caps).
    pub const FIRST_FREE: usize = NUM_INITIAL_CAPS + 1;
}

// ---------------------------------------------------------------------------
// seL4_IPCBuffer layout (x86_64)
// ---------------------------------------------------------------------------

/// Layout of the seL4 IPC buffer, matching `seL4_IPCBuffer` in the kernel.
///
/// The IPC buffer is a page (4 KiB) used for transferring larger messages and
/// capability references between threads.
///
/// On x86_64, the layout is:
/// ```c
/// typedef struct seL4_IPCBuffer_ {
///     seL4_MessageInfo tag;
///     seL4_Word msg[120];          // Message registers 0..119
///     seL4_Word userData;          // User-defined data word
///     seL4_Word caps_or_badges[3]; // Capability transfer / badge info
///     seL4_CPtr receiveCNode;      // CNode for receiving caps
///     seL4_CPtr receiveIndex;
///     seL4_Word receiveDepth;
/// } seL4_IPCBuffer;
/// ```
pub const IPC_BUFFER_MSG_REGS: usize = 120;
/// Size of the IPC buffer in bytes (one page).
pub const IPC_BUFFER_SIZE: usize = 4096;
/// IPC buffer must be page-aligned.
pub const IPC_BUFFER_ALIGN: usize = 4096;
