//! 文件控制常量

pub const O_RDONLY: u32 = 0;
pub const O_WRONLY: u32 = 1;
pub const O_RDWR: u32 = 2;
pub const O_CREAT: u32 = 64;
pub const O_TRUNC: u32 = 512;
pub const O_APPEND: u32 = 1024;
pub const O_DIRECTORY: u32 = 0x10000;

pub const AT_FDCWD: i32 = -100;
pub const AT_SYMLINK_NOFOLLOW: u32 = 0x100;
