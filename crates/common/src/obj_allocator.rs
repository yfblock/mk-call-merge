//! seL4 对象分配器
//!
//! 管理 seL4 能力（页面、页表、通知等）的分配和回收。

use alloc::vec::Vec;
use spin::Mutex;
use sel4_sys::*;

use crate::config::PAGE_SIZE;
use crate::slot::alloc_slot;

/// 对象分配器
pub struct ObjectAllocator {
    /// 可回收的未类型化内存 capability
    pub recycled: Vec<usize>,
    /// 已分配的未类型化内存列表 (slot, size_bits)
    pub untyped_list: Vec<(usize, u8)>,
}

impl ObjectAllocator {
    pub const fn new() -> Self {
        Self {
            recycled: Vec::new(),
            untyped_list: Vec::new(),
        }
    }

    /// 添加一个未类型化 capability
    pub fn add_untyped(&mut self, slot: usize, size_bits: u8) {
        self.untyped_list.push((slot, size_bits));
    }

    /// 分配一个页面 capability
    pub fn alloc_page(&mut self, bi: &BootInfo) -> Option<usize> {
        let slot = alloc_slot();
        let (untyped_slot, _) = bi.find_free_untyped(12)?; // 4KB = 2^12

        let err = seL4_Untyped_Retype(
            untyped_slot,
            ObjectType::Frame4K as usize,
            ObjectType::Frame4K.size_bits(),
            init_slots::CNODE,
            init_slots::CNODE,
            64,
            slot,
            1,
        );
        if err == 0 { Some(slot) } else { None }
    }

    /// 分配一个页表 capability
    pub fn alloc_page_table(&mut self, bi: &BootInfo) -> Option<usize> {
        let slot = alloc_slot();
        let (untyped_slot, _) = bi.find_free_untyped(12)?;

        let err = seL4_Untyped_Retype(
            untyped_slot,
            ObjectType::PageTable as usize,
            ObjectType::PageTable.size_bits(),
            init_slots::CNODE,
            init_slots::CNODE,
            64,
            slot,
            1,
        );
        if err == 0 { Some(slot) } else { None }
    }

    /// 分配一个通知 capability
    pub fn alloc_notification(&mut self, bi: &BootInfo) -> Option<usize> {
        let slot = alloc_slot();
        let (untyped_slot, _) = bi.find_free_untyped(12)?;

        let err = seL4_Untyped_Retype(
            untyped_slot,
            ObjectType::Notification as usize,
            ObjectType::Notification.size_bits(),
            init_slots::CNODE,
            init_slots::CNODE,
            64,
            slot,
            1,
        );
        if err == 0 { Some(slot) } else { None }
    }

    /// 回收一个 capability
    pub fn recycle(&mut self, slot: usize) {
        self.recycled.push(slot);
    }
}

/// 全局对象分配器
pub static OBJ_ALLOCATOR: Mutex<ObjectAllocator> = Mutex::new(ObjectAllocator::new());
