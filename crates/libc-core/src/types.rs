//! 基本类型定义

/// 时间值结构
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct TimeVal {
    pub sec: u64,
    pub usec: u64,
}

/// 时间规格结构
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct TimeSpec {
    pub sec: u64,
    pub nsec: u64,
}

/// Stat 结构
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Stat {
    pub dev: u64,
    pub ino: u64,
    pub nlink: u64,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub rdev: u64,
    pub size: i64,
    pub blksize: i64,
    pub blocks: i64,
    pub atime: TimeSpec,
    pub mtime: TimeSpec,
    pub ctime: TimeSpec,
}

/// Dirent64 结构
#[repr(C)]
pub struct Dirent64 {
    pub ino: u64,
    pub off: u64,
    pub reclen: u16,
    pub ftype: u8,
    pub name: [u8; 256],
}

impl Default for Dirent64 {
    fn default() -> Self {
        Self { ino: 0, off: 0, reclen: 0, ftype: 0, name: [0u8; 256] }
    }
}
