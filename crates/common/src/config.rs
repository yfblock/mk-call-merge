//! System configuration constants

/// Service boot stack top address
pub const SERVICE_BOOT_STACK_TOP: usize = 0x1_0000_0000;

/// Service boot stack size
pub const SERVICE_BOOT_STACK_SIZE: usize = 0x1_0000;

/// Default heap size for services
pub const SERVICE_HEAP_SIZE: usize = 0x10_0000;

/// Page size
pub const PAGE_SIZE: usize = 0x1000;

/// Page mask
pub const PAGE_MASK: usize = !0xfff;

/// Default CSpace radix bits
pub const CNODE_RADIX_BITS: usize = 12;

/// IPC buffer size
pub const IPC_BUFFER_SIZE: usize = core::mem::size_of::<sel4_sys::IpcBuffer>();

/// Default custom capability slot
pub const DEFAULT_CUSTOM_SLOT: u64 = 26;

/// Default empty slot index
pub const DEFAULT_EMPTY_SLOT_INDEX: usize = 32;

/// Stack alignment
pub const STACK_ALIGN_SIZE: usize = 16;

/// IPC data length
pub const IPC_DATA_LEN: usize = 120 * 8;

/// Register length
pub const REG_LEN: usize = core::mem::size_of::<usize>();

/// Default thread notification slot
pub const DEFAULT_THREAD_NOTIFICATION: u64 = 17;

/// DMA address start
pub const DMA_ADDR_START: usize = 0x1_0000_3000;

/// Share page start
pub const SHARE_PAGE_START: usize = 0x1_001F_0000;

/// IO port base for x86
pub const IO_PORT_BASE: u16 = 0x3f8;

/// Linux App 使用的 CNode bits
pub const LINUX_APP_CNODE_RADIX_BITS: usize = 6;
