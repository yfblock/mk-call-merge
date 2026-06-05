//! 物理页抽象
//!
//! 提供 PhysPage 结构，封装 seL4 物理页能力和页内容访问。

use core::ops::{Deref, DerefMut};
use spin::Mutex;

use crate::config::PAGE_SIZE;

/// 物理页封装
///
/// 包含 seL4 页 capability slot 和页内容访问。
/// 通过 Mutex 保护页内容的并发访问。
pub struct PhysPage {
    /// seL4 页 capability slot
    cap: usize,
    /// 页内容（在内核地址空间中映射的地址）
    content: Mutex<&'static mut [u8; PAGE_SIZE]>,
}

impl PhysPage {
    /// 创建一个新的 PhysPage
    ///
    /// # Safety
    /// `content_ptr` 必须指向一个有效的、已映射的 4KB 页
    pub unsafe fn new(cap: usize, content_ptr: *mut u8) -> Self {
        Self {
            cap,
            content: Mutex::new(unsafe { &mut *(content_ptr as *mut [u8; PAGE_SIZE]) }),
        }
    }

    /// 获取 capability slot
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// 获取页内容的锁
    pub fn lock(&self) -> spin::MutexGuard<'_, &'static mut [u8; PAGE_SIZE]> {
        self.content.lock()
    }

    /// 获取页内容的原始指针
    pub fn as_ptr(&self) -> *const u8 {
        self.content.lock().as_ptr()
    }

    /// 获取页内容的可变原始指针
    pub fn as_mut_ptr(&self) -> *mut u8 {
        self.content.lock().as_mut_ptr()
    }

    /// 读取 usize 值
    pub fn read_usize(&self, offset: usize) -> usize {
        let content = self.content.lock();
        let bytes = &content[offset..offset + core::mem::size_of::<usize>()];
        usize::from_le_bytes(bytes.try_into().unwrap())
    }

    /// 写入 usize 值
    pub fn write_usize(&self, offset: usize, value: usize) {
        let mut content = self.content.lock();
        let bytes = value.to_le_bytes();
        content[offset..offset + core::mem::size_of::<usize>()].copy_from_slice(&bytes);
    }
}

// PhysPage is Send + Sync because the Mutex protects concurrent access
unsafe impl Send for PhysPage {}
unsafe impl Sync for PhysPage {}
