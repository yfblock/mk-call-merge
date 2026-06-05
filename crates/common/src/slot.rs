//! Slot 管理模块
//!
//! 提供 CNode slot 的分配和回收，用于管理 seL4 能力。

use core::ops::Range;
use spin::Mutex;

/// Slot 管理器
pub struct SlotManager {
    /// 下一个可分配的 slot
    next: usize,
    /// 空闲 slot 范围结束
    end: usize,
    /// 已回收的 slot 列表
    recycled: alloc::vec::Vec<usize>,
}

impl SlotManager {
    pub const fn empty() -> Self {
        Self {
            next: 0,
            end: 0,
            recycled: alloc::vec::Vec::new(),
        }
    }

    /// 初始化空闲 slot 范围
    pub fn init_empty_slots(&mut self, range: Range<usize>) {
        self.next = range.start;
        self.end = range.end;
    }

    /// 可用 slot 数量
    pub fn available(&self) -> usize {
        self.recycled.len() + if self.end > self.next { self.end - self.next } else { 0 }
    }

    /// 分配一个 slot
    pub fn alloc_slot(&mut self) -> usize {
        if let Some(slot) = self.recycled.pop() {
            return slot;
        }
        if self.next < self.end {
            let slot = self.next;
            self.next += 1;
            slot
        } else {
            panic!("No available slots");
        }
    }

    /// 回收一个 slot
    pub fn recycle_slot(&mut self, slot: usize) {
        self.recycled.push(slot);
    }

    /// 下一个范围的起始位置
    pub fn next_range_start(&self) -> usize {
        self.end
    }

    /// 扩展 slot 范围
    pub fn extend(&mut self, count: usize) {
        self.end += count;
    }
}

/// 全局 slot 管理器
pub static SLOT_MANAGER: Mutex<SlotManager> = Mutex::new(SlotManager::empty());

/// Slot 边缘处理回调
type SlotEdgeHandler = fn(usize);
static SLOT_EDGE_HANDLER: Mutex<Option<SlotEdgeHandler>> = Mutex::new(None);

/// 初始化 slot 管理器
pub fn init(empty_slots: Range<usize>) {
    SLOT_MANAGER.lock().init_empty_slots(empty_slots);
}

/// 设置 slot 边缘处理函数
pub fn init_slot_edge_handler(handler: SlotEdgeHandler) {
    *SLOT_EDGE_HANDLER.lock() = Some(handler);
}

/// 分配一个 slot
pub fn alloc_slot() -> usize {
    let mut sm = SLOT_MANAGER.lock();
    if sm.available() == 0 {
        if let Some(handler) = *SLOT_EDGE_HANDLER.lock() {
            handler(sm.next_range_start());
        }
        sm.extend(0x1000);
    }
    sm.alloc_slot()
}

/// 回收一个 slot
pub fn recycle_slot(slot: usize) {
    SLOT_MANAGER.lock().recycle_slot(slot);
}
