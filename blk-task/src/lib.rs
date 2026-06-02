#![no_std]

extern crate alloc;

use core::cell::UnsafeCell;

/// 块设备接口 trait
pub trait BlockIface {
    /// 读取块数据到缓冲区
    fn read_block(&self, block_id: usize, buf: &mut [u8]);
    /// 从缓冲区写入块数据
    fn write_block(&self, block_id: usize, buf: &[u8]);
    /// 返回设备容量（字节）
    fn capacity(&self) -> u64;
}

/// 块大小：512 字节
pub const BLOCK_SIZE: usize = 512;

/// Ramdisk 容量：4MB = 8192 blocks
pub const RAMDISK_SIZE: usize = 4 * 1024 * 1024;
pub const RAMDISK_BLOCKS: usize = RAMDISK_SIZE / BLOCK_SIZE;

/// 线程安全的静态缓冲区包装
struct SyncBuffer(UnsafeCell<[u8; RAMDISK_SIZE]>);
unsafe impl Sync for SyncBuffer {}

static RAMDISK_BUF: SyncBuffer = SyncBuffer(UnsafeCell::new([0; RAMDISK_SIZE]));

/// 全局 ramdisk 实例（供 lwext4-task 等直接调用）
pub static BLK: RamdiskBlkImpl = RamdiskBlkImpl { data: &RAMDISK_BUF };

/// Ramdisk 实现：内存盘
pub struct RamdiskBlkImpl {
    data: &'static SyncBuffer,
}

impl RamdiskBlkImpl {
    pub fn new() -> Self {
        Self { data: &RAMDISK_BUF }
    }

    /// 从镜像数据初始化 ramdisk
    pub fn init_from_image(&self, image: &[u8]) {
        let data = unsafe { &mut *self.data.0.get() };
        let len = image.len().min(RAMDISK_SIZE);
        data[..len].copy_from_slice(&image[..len]);
    }
}

impl BlockIface for RamdiskBlkImpl {
    fn read_block(&self, block_id: usize, buf: &mut [u8]) {
        let data = unsafe { &*self.data.0.get() };
        let start = block_id * BLOCK_SIZE;
        let len = buf.len().min(BLOCK_SIZE).min(RAMDISK_SIZE - start);
        buf[..len].copy_from_slice(&data[start..start + len]);
    }

    fn write_block(&self, block_id: usize, buf: &[u8]) {
        let data = unsafe { &mut *self.data.0.get() };
        let start = block_id * BLOCK_SIZE;
        let len = buf.len().min(BLOCK_SIZE).min(RAMDISK_SIZE - start);
        data[start..start + len].copy_from_slice(&buf[..len]);
    }

    fn capacity(&self) -> u64 {
        RAMDISK_SIZE as u64
    }
}
