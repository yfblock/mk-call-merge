//! seL4 BootInfo parsing.
//!
//! The kernel provides a `seL4_BootInfo` structure to the root task at boot.
//! This module parses that structure and provides typed accessors for
//! capabilities, untyped memory regions, device regions, and IRQ mappings.

use core::fmt;

// ---------------------------------------------------------------------------
// Raw BootInfo layout (matching seL4_BootInfo in the kernel)
// ---------------------------------------------------------------------------

/// Maximum number of untyped descriptors in the bootinfo.
pub const MAX_UNTYPED_CAPS: usize = 230;

/// Raw `seL4_BootInfo` structure as provided by the kernel.
///
/// This is a `#[repr(C)]` struct that directly maps to the kernel's binary
/// layout on x86_64. It is placed in memory by the kernel before the root
/// task starts.
#[repr(C)]
pub struct BootInfoRaw {
    /// Length of any additional bootinfo information (in bytes).
    pub extra_len: usize,
    /// Node ID (0 if uniprocessor).
    pub node_id: usize,
    /// Number of seL4 nodes (1 if uniprocessor).
    pub num_nodes: usize,
    /// Number of IOMMU PT levels (0 if no IOMMU support).
    pub num_iopt_levels: usize,
    /// Pointer to initial thread's IPC buffer.
    pub ipc_buffer: *mut u8,
    /// Empty slots (null caps).
    pub empty: SlotRegion,
    /// Shared-frame caps.
    pub shared_frames: SlotRegion,
    /// Userland-image frame caps.
    pub user_image_frames: SlotRegion,
    /// Userland-image paging structure caps.
    pub user_image_paging: SlotRegion,
    /// IOSpace caps (ARM SMMU).
    pub io_space_caps: SlotRegion,
    /// Caps for extra bootinfo pages.
    pub extra_bi_pages: SlotRegion,
    /// Root CNode size (2^n slots).
    pub init_thread_cnode_size_bits: usize,
    /// Initial thread's domain ID.
    pub init_thread_domain: usize,
    /// Untyped-object capability slot range.
    pub untyped: SlotRegion,
    /// Information about each untyped capability.
    pub untyped_list: [UntypedDesc; MAX_UNTYPED_CAPS],
}

/// A slot region descriptor (start..end range of CNode slots).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SlotRegion {
    /// First CNode slot position in region.
    pub start: usize,
    /// First CNode slot position AFTER region.
    pub end: usize,
}

impl SlotRegion {
    /// Number of slots in this region.
    pub fn count(&self) -> usize {
        if self.end > self.start {
            self.end - self.start
        } else {
            0
        }
    }
}

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

    /// Get the untyped slot region (start..end of untyped capability slots).
    pub fn untyped(&self) -> SlotRegion {
        self.raw().untyped
    }

    /// Get the first untyped capability slot.
    pub fn untyped_start(&self) -> usize {
        self.raw().untyped.start
    }

    /// Get the number of untyped capabilities.
    pub fn untyped_count(&self) -> usize {
        self.raw().untyped.count()
    }

    /// Get the root CNode size bits.
    pub fn cnode_size_bits(&self) -> usize {
        self.raw().init_thread_cnode_size_bits
    }

    /// Get the IPC buffer pointer.
    pub fn ipc_buffer(&self) -> *mut u8 {
        self.raw().ipc_buffer
    }

    /// Get the node ID.
    pub fn node_id(&self) -> usize {
        self.raw().node_id
    }

    /// Get the empty slot region (slots available for user allocation).
    pub fn empty(&self) -> SlotRegion {
        self.raw().empty
    }

    /// Get the untyped descriptor for a given index.
    pub fn untyped_desc(&self, index: usize) -> &UntypedDesc {
        &self.raw().untyped_list[index]
    }

    /// Find the first non-device untyped cap that is large enough.
    /// Returns the (slot, size_bits) of the untyped, or None if not found.
    pub fn find_free_untyped(&self, min_size_bits: u8) -> Option<(usize, u8)> {
        let start = self.untyped_start();
        let count = self.untyped_count();
        for i in 0..count {
            let desc = self.untyped_desc(i);
            if desc.is_device == 0 && desc.size_bits >= min_size_bits {
                return Some((start + i, desc.size_bits));
            }
        }
        None
    }
}

impl fmt::Debug for BootInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BootInfo")
            .field("node_id", &self.node_id())
            .field("untyped_start", &self.untyped_start())
            .field("untyped_count", &self.untyped_count())
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
