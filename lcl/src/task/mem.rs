//! Task memory management - ported from kernel-thread

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;
use common::config::PAGE_SIZE;
use core::cmp;

use crate::consts::task::{DEF_HEAP_ADDR, DEF_STACK_BOTTOM, DEF_STACK_TOP};

/// Task memory info
pub struct TaskMemInfo {
    /// Mapped pages: vaddr -> slot
    pub mapped_page: alloc::collections::BTreeMap<usize, usize>,
    /// Mapped page tables
    pub mapped_pt: Vec<usize>,
    /// Current heap address
    pub heap: usize,
    /// Page data cache: vaddr -> page content
    pub page_data: alloc::collections::BTreeMap<usize, [u8; 4096]>,
}

impl Default for TaskMemInfo {
    fn default() -> Self {
        Self {
            mapped_page: alloc::collections::BTreeMap::new(),
            mapped_pt: Vec::new(),
            heap: DEF_HEAP_ADDR,
            page_data: alloc::collections::BTreeMap::new(),
        }
    }
}

impl super::Sel4Task {
    /// brk syscall - adjust heap
    pub fn brk(&self, value: usize) -> usize {
        let mut mem_info = self.mem.lock();
        if value == 0 {
            return mem_info.heap;
        }
        let origin = mem_info.heap;
        mem_info.heap = value;
        drop(mem_info);
        // TODO: map new pages for heap growth
        value
    }

    /// Read an instruction (u32) from task's address space
    pub fn read_ins(&self, vaddr: usize) -> Option<u32> {
        self.mem
            .lock()
            .mapped_page
            .get(&(vaddr / PAGE_SIZE * PAGE_SIZE))
            .map(|_page| {
                // In real implementation, would read from mapped page
                0
            })
    }

    /// Read bytes from task's address space
    pub fn read_bytes_task(&self, mut vaddr: usize, len: usize) -> Option<Vec<u8>> {
        let mut data = Vec::new();
        let mem_info = self.mem.lock();
        let vaddr_end = vaddr + len;
        while vaddr < vaddr_end {
            let _page = mem_info.mapped_page.get(&(vaddr / PAGE_SIZE * PAGE_SIZE))?;
            let offset = vaddr % PAGE_SIZE;
            let rsize = cmp::min(PAGE_SIZE - offset, vaddr_end - vaddr);
            // In real implementation, would read from page
            data.extend_from_slice(&vec![0u8; rsize]);
            vaddr += rsize;
        }
        Some(data)
    }

    /// Read a C string from task's address space
    pub fn read_cstr(&self, mut vaddr: usize) -> Option<Vec<u8>> {
        let mut data = Vec::new();
        let mem_info = self.mem.lock();
        loop {
            let _page = mem_info.mapped_page.get(&(vaddr / PAGE_SIZE * PAGE_SIZE))?;
            let offset = vaddr % PAGE_SIZE;
            // Read up to PAGE_SIZE bytes
            let rsize = PAGE_SIZE - offset;
            // In real implementation, would scan for null terminator
            data.extend_from_slice(&vec![0u8; rsize]);
            vaddr += rsize;
            if data.len() > 4096 {
                break;
            }
        }
        Some(data)
    }

    /// Write bytes to task's address space
    pub fn write_bytes_task(&self, mut vaddr: usize, data: &[u8]) -> Option<()> {
        let mem_info = self.mem.lock();
        let vaddr_end = vaddr + data.len();
        while vaddr < vaddr_end {
            let _page = mem_info.mapped_page.get(&(vaddr / PAGE_SIZE * PAGE_SIZE))?;
            let offset = vaddr % PAGE_SIZE;
            let rsize = cmp::min(PAGE_SIZE - offset, vaddr_end - vaddr);
            // In real implementation, would write to page
            let _ = offset;
            let _ = rsize;
            vaddr += rsize;
        }
        Some(())
    }

    /// Check if address is in stack range and map if needed
    pub fn check_addr(&self, vaddr: usize, size: usize) {
        let bottom = vaddr / PAGE_SIZE * PAGE_SIZE;
        let top = (vaddr + size).div_ceil(PAGE_SIZE) * PAGE_SIZE;
        for addr in (bottom..top).step_by(PAGE_SIZE) {
            if self.mem.lock().mapped_page.contains_key(&addr) {
                continue;
            }
            if (DEF_STACK_BOTTOM..DEF_STACK_TOP).contains(&addr) {
                self.map_blank_page_simple(addr);
            }
        }
    }

    /// Clear all mapped memory
    pub fn clear_mapped(&self) {
        self.mem.lock().mapped_page.clear();
        self.mem.lock().mapped_pt.clear();
    }
}
