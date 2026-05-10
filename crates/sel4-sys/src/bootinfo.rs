//! seL4 BootInfo parsing.
//!
//! The kernel provides a `seL4_BootInfo` structure to the root task at boot.
//! This module parses that structure and provides typed accessors for
//! capabilities, untyped memory regions, device regions, and IRQ mappings.

use core::fmt;

// ---------------------------------------------------------------------------
// Raw BootInfo layout (matching seL4_BootInfo in the kernel)
// ---------------------------------------------------------------------------

/// Raw `seL4_BootInfo` structure as provided by the kernel.
///
/// This is a `#[repr(C)]` struct that directly maps to the kernel's binary
/// layout. It is placed in memory by the kernel loader before the root task
/// starts.
#[repr(C)]
pub struct BootInfoRaw {
    /// ID [0] is an information identifier word. For x86_64, expected to be
    /// 0xffffff7f (SEL4_BOOTINFO_HEADER_PADDING) or similar.
    pub extra_len: u32,
    /// BootInfo ID. Should be `SEL4_BOOTINFO_HEADER_X86_VBE` or equivalent.
    pub node_id: u32,
    /// Number of capability slots in the root CNode.
    pub num_iopt_levels: u32,
    /// Number of IOAPIC IRQs.
    pub num_ioapic: u32,
    /// Offset of IPC buffer CPtr.
    pub ipc_buffer: u32,
    /// Empty slots (core has nothing).
    pub empty: u32,
    /// Shared frames (core has nothing).
    pub shared_frames: u32,
    /// User image frames (core has nothing).
    pub user_image_frames: u32,
    /// User image PTs (core has nothing).
    pub user_image_pts: u32,
    /// Number of untyped memory regions.
    pub untyped_list: u32,
    /// Size of the untyped list (in words).
    pub untyped_list_size: u32,
    /// Physical address of the untyped list.
    pub untyped_list_paddr: u32,
    /// Number of device regions.
    pub device_list: u32,
    /// Size of the device region list.
    pub device_list_size: u32,
    /// Physical address of the device region list.
    pub device_list_paddr: u32,
    /// Number of IOAPIC structures.
    pub ioapic_list: u32,
    /// Size of IOAPIC list.
    pub ioapic_list_size: u32,
    /// Physical address of IOAPIC list.
    pub ioapic_list_paddr: u32,
}

/// BootInfo header ID expected for x86_64.
pub const SEL4_BOOTINFO_HEADER_X86_VBE: u32 = 0x01000005;
/// Alternative BootInfo header padding expected on some platforms.
pub const SEL4_BOOTINFO_HEADER_PADDING: u32 = 0xffffff7f;

/// Untyped memory descriptor from the bootinfo.
///
/// Each untyped region describes a contiguous range of physical memory
/// available for allocation.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UntypedDesc {
    /// Physical address of the untyped memory region.
    pub paddr: u64,
    /// Size bits of the region (log2 of size in bytes).
    pub size_bits: u8,
    /// Whether this region is device memory.
    pub is_device: u8,
    /// Padding for alignment.
    pub padding: [u8; 6],
}

/// Device region descriptor from the bootinfo.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DeviceDesc {
    /// Physical address of the device region.
    pub paddr: u64,
    /// Size of the device region in bytes.
    pub size: u32,
    /// Padding for alignment.
    pub padding: [u8; 4],
}

/// IOAPIC descriptor from the bootinfo (x86_64 only).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct IoApicDesc {
    /// IOAPIC ID.
    pub id: u32,
    /// IOAPIC base physical address.
    pub paddr: u32,
    /// Global system interrupt base.
    pub gsi_base: u32,
}

// ---------------------------------------------------------------------------
// Typed BootInfo wrapper
// ---------------------------------------------------------------------------

/// A typed, safe wrapper around `BootInfoRaw`.
///
/// Provides methods to iterate over untyped regions, device regions, etc.
pub struct BootInfo {
    /// Pointer to the raw bootinfo structure.
    raw: *const BootInfoRaw,
}

// Safety: BootInfo is read-only and the kernel ensures the data is valid
// for the lifetime of the root task.
unsafe impl Send for BootInfo {}
unsafe impl Sync for BootInfo {}

impl BootInfo {
    /// Create a `BootInfo` from a raw pointer.
    ///
    /// # Safety
    ///
    /// The pointer must point to a valid, kernel-populated `BootInfoRaw`
    /// structure that remains valid for the lifetime of this wrapper.
    pub unsafe fn from_raw(raw: *const BootInfoRaw) -> Self {
        Self { raw }
    }

    /// Get a reference to the raw bootinfo structure.
    fn raw(&self) -> &BootInfoRaw {
        unsafe { &*self.raw }
    }

    /// Get the number of IOAPIC IRQs.
    pub fn num_ioapic(&self) -> u32 {
        self.raw().num_ioapic
    }

    /// Get the number of capability slots in the root CNode.
    pub fn num_iopt_levels(&self) -> u32 {
        self.raw().num_iopt_levels
    }

    /// Get the IPC buffer capability pointer.
    pub fn ipc_buffer_slot(&self) -> u32 {
        self.raw().ipc_buffer
    }

    /// Get the number of untyped memory regions.
    pub fn untyped_count(&self) -> u32 {
        self.raw().untyped_list
    }

    /// Get an iterator over the untyped memory descriptors.
    pub fn untyped_list(&self) -> UntypedIter {
        let count = self.raw().untyped_list as usize;
        let ptr = self.raw().untyped_list_paddr as *const UntypedDesc;
        UntypedIter {
            ptr,
            count,
            index: 0,
        }
    }

    /// Get the number of device regions.
    pub fn device_count(&self) -> u32 {
        self.raw().device_list
    }

    /// Get an iterator over the device region descriptors.
    pub fn device_list(&self) -> DeviceIter {
        let count = self.raw().device_list as usize;
        let ptr = self.raw().device_list_paddr as *const DeviceDesc;
        DeviceIter {
            ptr,
            count,
            index: 0,
        }
    }

    /// Get an iterator over the IOAPIC descriptors.
    pub fn ioapic_list(&self) -> IoApicIter {
        let count = self.raw().ioapic_list as usize;
        let ptr = self.raw().ioapic_list_paddr as *const IoApicDesc;
        IoApicIter {
            ptr,
            count,
            index: 0,
        }
    }
}

impl fmt::Debug for BootInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BootInfo")
            .field("num_ioapic", &self.num_ioapic())
            .field("untyped_count", &self.untyped_count())
            .field("device_count", &self.device_count())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Iterators
// ---------------------------------------------------------------------------

/// Iterator over untyped memory descriptors.
pub struct UntypedIter {
    ptr: *const UntypedDesc,
    count: usize,
    index: usize,
}

impl Iterator for UntypedIter {
    type Item = UntypedDesc;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.count {
            return None;
        }
        let desc = unsafe { *self.ptr.add(self.index) };
        self.index += 1;
        Some(desc)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.count - self.index;
        (remaining, Some(remaining))
    }
}

/// Iterator over device region descriptors.
pub struct DeviceIter {
    ptr: *const DeviceDesc,
    count: usize,
    index: usize,
}

impl Iterator for DeviceIter {
    type Item = DeviceDesc;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.count {
            return None;
        }
        let desc = unsafe { *self.ptr.add(self.index) };
        self.index += 1;
        Some(desc)
    }
}

/// Iterator over IOAPIC descriptors.
pub struct IoApicIter {
    ptr: *const IoApicDesc,
    count: usize,
    index: usize,
}

impl Iterator for IoApicIter {
    type Item = IoApicDesc;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.count {
            return None;
        }
        let desc = unsafe { *self.ptr.add(self.index) };
        self.index += 1;
        Some(desc)
    }
}
