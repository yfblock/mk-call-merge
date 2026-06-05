//! Task management - Sel4Task struct and ELF loading for x86_64

pub mod file;
pub mod info;
pub mod init;
pub mod loader;
pub mod mem;
pub mod pcb;
pub mod signal;
pub mod runner;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use sel4_sys::*;
use common::config::PAGE_SIZE;

use crate::consts::task::*;
use crate::utils::obj::OBJ_ALLOCATOR;
use crate::child_test::{FutexTable, wake_hangs, futex_wake};
use crate::task::mem::TaskMemInfo;

/// Task ID type
pub type TaskId = usize;

/// Poll wake event types
pub enum PollWakeEvent {
    Signal(usize),
    Timer,
    Blocking,
}

/// Sel4Task - core task structure
pub struct Sel4Task {
    pub pid: TaskId,
    pub ppid: TaskId,
    pub pgid: TaskId,
    pub tid: TaskId,
    pub tcb: usize,
    pub cnode: usize,
    pub vspace: usize,
    pub exit: Mutex<Option<u32>>,
    pub futex_table: Arc<Mutex<FutexTable>>,
    pub clear_child_tid: Mutex<usize>,
    pub info: Mutex<TaskInfo>,
    pub mem: Mutex<TaskMemInfo>,
    pub signal: Mutex<crate::task::signal::TaskSignal>,
}

/// Task info
#[derive(Default, Clone)]
pub struct TaskInfo {
    pub entry: usize,
    pub task_vm_end: usize,
    pub args: Vec<String>,
}

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

impl Sel4Task {
    /// Create a new task
    pub fn new(bi: &BootInfo) -> Option<Self> {
        let tid = ID_COUNTER.fetch_add(1, Ordering::SeqCst) as usize;

        // Allocate slots for TCB, CNode, VSpace
        let (tcb_slot, cnode_slot, vspace_slot);
        {
            let mut sm = crate::utils::obj::OBJ_ALLOCATOR.lock();
            tcb_slot = sm.alloc().unwrap();
            cnode_slot = sm.alloc().unwrap();
            vspace_slot = sm.alloc().unwrap();
        }

        // Find untyped for creating objects
        let (untyped_slot, _) = bi.find_free_untyped(12)?;

        // Create TCB
        let err = seL4_Untyped_Retype(
            untyped_slot, ObjectType::TCB as usize,
            ObjectType::TCB.size_bits(), init_slots::CNODE, init_slots::CNODE,
            64, tcb_slot, 1,
        );
        if err != 0 { return None; }

        // Create CNode
        let err = seL4_Untyped_Retype(
            untyped_slot, ObjectType::CNode as usize,
            6, // radix
            init_slots::CNODE, init_slots::CNODE,
            64, cnode_slot, 1,
        );
        if err != 0 { return None; }

        Some(Sel4Task {
            pid: tid,
            ppid: 1,
            pgid: 0,
            tid,
            tcb: tcb_slot,
            cnode: cnode_slot,
            vspace: vspace_slot,
            exit: Mutex::new(None),
            futex_table: Arc::new(Mutex::new(Vec::new())),
            clear_child_tid: Mutex::new(0),
            info: Mutex::new(TaskInfo::default()),
            mem: Mutex::new(TaskMemInfo::default()),
            signal: Mutex::new(crate::task::signal::TaskSignal::new()),
        })
    }

    /// Map a blank page at the given virtual address
    /// Uses page_data cache for in-memory storage
    pub fn map_blank_page_simple(&self, vaddr: usize) -> Option<usize> {
        let vaddr = vaddr & !(PAGE_SIZE - 1);
        let mut mem = self.mem.lock();
        mem.mapped_page.entry(vaddr).or_insert(0);
        mem.page_data.entry(vaddr).or_insert_with(|| [0u8; PAGE_SIZE]);
        Some(0)
    }

    /// Map a blank page with real seL4 frame capability
    pub fn map_blank_page(&self, vaddr: usize, bi: &BootInfo) -> Option<usize> {
        let vaddr = vaddr & !(PAGE_SIZE - 1);
        let (page_slot, untyped_slot);
        {
            let mut sm = crate::utils::obj::OBJ_ALLOCATOR.lock();
            page_slot = sm.alloc()?;
            let (ut, _) = bi.find_free_untyped(12)?;
            untyped_slot = ut;
        }

        // Create frame
        let err = seL4_Untyped_Retype(
            untyped_slot, ObjectType::Frame4K as usize,
            ObjectType::Frame4K.size_bits(), init_slots::CNODE, init_slots::CNODE,
            64, page_slot, 1,
        );
        if err != 0 { return None; }

        // Map frame
        let err = seL4_Frame_Map(
            page_slot, self.vspace, vaddr, CapRights::ALL.bits(), 0,
        );
        if err != 0 { return None; }

        self.mem.lock().mapped_page.insert(vaddr, page_slot);
        Some(page_slot)
    }

    /// Read bytes from task's address space (via mapped page)
    pub fn read_bytes(&self, addr: usize, buf: &mut [u8]) -> bool {
        let page_vaddr = addr & !(PAGE_SIZE - 1);
        let offset = addr & (PAGE_SIZE - 1);
        let mem = self.mem.lock();

        if let Some(page) = mem.page_data.get(&page_vaddr) {
            let len = buf.len().min(PAGE_SIZE - offset);
            buf[..len].copy_from_slice(&page[offset..offset + len]);
            true
        } else {
            false
        }
    }

    /// Read a u32 from task's address space
    pub fn read_bytes_u32(&self, addr: usize) -> Option<u32> {
        let mut buf = [0u8; 4];
        if self.read_bytes(addr, &mut buf) {
            Some(u32::from_le_bytes(buf))
        } else {
            None
        }
    }

    /// Write bytes to task's address space (supports cross-page writes)
    pub fn write_bytes(&self, mut addr: usize, data: &[u8]) -> bool {
        let mut remaining = data;
        while !remaining.is_empty() {
            let page_vaddr = addr & !(PAGE_SIZE - 1);
            let offset = addr & (PAGE_SIZE - 1);
            let len = remaining.len().min(PAGE_SIZE - offset);

            let mut mem = self.mem.lock();
            if let Some(page) = mem.page_data.get_mut(&page_vaddr) {
                page[offset..offset + len].copy_from_slice(&remaining[..len]);
            } else {
                let mut page = [0u8; PAGE_SIZE];
                page[offset..offset + len].copy_from_slice(&remaining[..len]);
                mem.mapped_page.insert(page_vaddr, 0);
                mem.page_data.insert(page_vaddr, page);
            }

            addr += len;
            remaining = &remaining[len..];
        }
        true
    }

    /// Find free virtual address area
    pub fn find_free_area(&self, start: usize, size: usize) -> usize {
        let mut last_addr = self.info.lock().task_vm_end.max(start);
        let mem = self.mem.lock();
        for vaddr in mem.mapped_page.keys() {
            if last_addr + size <= *vaddr {
                return last_addr;
            }
            last_addr = *vaddr + PAGE_SIZE;
        }
        last_addr
    }

    /// Load an ELF binary into the task's address space
    ///
    /// On x86_64, patches `syscall` instructions (0x0f05) to `0xdeadbeef`
    /// so they trigger a user exception that we can intercept as syscalls.
    pub fn load_elf(&self, elf_data: &[u8]) -> Option<usize> {
        if elf_data.len() < 64 {
            return None;
        }
        if &elf_data[0..4] != b"\x7fELF" {
            return None;
        }
        if elf_data[4] != 2 {
            return None;
        }

        let e_phoff = u64::from_le_bytes(elf_data[32..40].try_into().ok()?) as usize;
        let e_phentsize = u16::from_le_bytes(elf_data[54..56].try_into().ok()?) as usize;
        let e_phnum = u16::from_le_bytes(elf_data[56..58].try_into().ok()?) as usize;
        let e_entry = u64::from_le_bytes(elf_data[24..32].try_into().ok()?) as usize;

        let mut max_addr = 0usize;

        // Load PT_LOAD segments
        for i in 0..e_phnum {
            let ph_off = e_phoff + i * e_phentsize;
            if ph_off + 56 > elf_data.len() {
                break;
            }
            let p_type = u32::from_le_bytes(elf_data[ph_off..ph_off + 4].try_into().ok()?);
            let p_offset = u64::from_le_bytes(elf_data[ph_off + 8..ph_off + 16].try_into().ok()?) as usize;
            let p_vaddr = u64::from_le_bytes(elf_data[ph_off + 16..ph_off + 24].try_into().ok()?) as usize;
            let p_filesz = u64::from_le_bytes(elf_data[ph_off + 32..ph_off + 40].try_into().ok()?) as usize;
            let p_memsz = u64::from_le_bytes(elf_data[ph_off + 40..ph_off + 48].try_into().ok()?) as usize;

            if p_type != 1 {
                continue;
            }

            let end_addr = p_vaddr + p_memsz;
            if end_addr > max_addr {
                max_addr = end_addr;
            }

            // Map pages for this segment
            let start_page = p_vaddr & !(PAGE_SIZE - 1);
            let end_page = (end_addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
            for page in (start_page..end_page).step_by(PAGE_SIZE) {
                self.map_blank_page_simple(page);
            }

            // Write segment data to mapped pages
            if p_filesz > 0 {
                let data = &elf_data[p_offset..p_offset + p_filesz];
                self.write_bytes(p_vaddr, data);
            }

            // Patch syscall instructions (0x0f05) to 0xdeadbeef
            if p_filesz > 0 {
                let data = &elf_data[p_offset..p_offset + p_filesz];
                for j in 0..data.len().saturating_sub(1) {
                    if data[j] == 0x0f && data[j + 1] == 0x05 {
                        // Found syscall instruction, patch to deadbeef
                        let patch_addr = p_vaddr + j;
                        self.write_bytes(patch_addr, &[0xef, 0xbe, 0xad, 0xde]);
                    }
                }
            }
        }

        self.info.lock().entry = e_entry;
        self.info.lock().task_vm_end = (max_addr + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        Some(e_entry)
    }

    /// Exit the task
    pub fn exit_with(&self, code: u32) {
        *self.exit.lock() = Some(code);
        wake_hangs(self);

        let uaddr = *self.clear_child_tid.lock();
        if uaddr != 0 {
            self.write_bytes(uaddr, &0u32.to_le_bytes());
            futex_wake(self.futex_table.clone(), uaddr, 1);
        }

        // Release capabilities
        let _ = seL4_CNode_Revoke(init_slots::CNODE, self.tcb, 64);
        let _ = seL4_CNode_Delete(init_slots::CNODE, self.tcb, 64);
        let _ = seL4_CNode_Revoke(init_slots::CNODE, self.cnode, 64);
        let _ = seL4_CNode_Delete(init_slots::CNODE, self.cnode, 64);
    }
}
