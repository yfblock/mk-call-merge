//! Page mapping utilities - ported from kernel-thread

use common::config::PAGE_SIZE;
use sel4_sys::*;

/// Map a physical page into the kernel's address space
pub fn map_page_self(page_slot: usize, vaddr: usize, vspace: usize) -> bool {
    let vaddr = vaddr & !(PAGE_SIZE - 1);
    let err = seL4_Frame_Map(page_slot, vspace, vaddr, CapRights::ALL.bits(), 0);
    err == 0
}

/// Unmap a page
pub fn unmap_page(page_slot: usize) -> bool {
    let err = seL4_Frame_Unmap(page_slot);
    err == 0
}

/// Map a page table
pub fn map_page_table(pt_slot: usize, vaddr: usize, vspace: usize) -> bool {
    let vaddr = vaddr & !(PAGE_SIZE - 1);
    let err = seL4_PageTable_Map(pt_slot, vspace, vaddr, 0);
    err == 0
}
