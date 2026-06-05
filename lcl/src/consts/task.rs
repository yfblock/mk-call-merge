//! Task memory layout constants

/// Heap start address
pub const DEF_HEAP_ADDR: usize = 0x7000_0000;

/// Stack top address
pub const DEF_STACK_TOP: usize = 0x2_0000_0000;

/// Stack bottom address
pub const DEF_STACK_BOTTOM: usize = 0x1_F000_0000;

/// User space base
pub const USPACE_BASE: usize = 0x1000;

/// VDSO area address
pub const VDSO_ADDR: usize = 0x4_0000_0000;

/// VDSO area size
pub const VDSO_AREA_SIZE: usize = 0x1000;

/// Page copy temp address
pub const PAGE_COPY_TEMP: usize = 0x8_0000_0000;

/// Default stack size
pub const DEFAULT_STACK_SIZE: usize = 0x100000; // 1MB
