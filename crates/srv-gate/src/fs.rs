//! Filesystem interface

/// Filesystem interface trait
pub trait FSIface: Sync + Send {
    fn init(&mut self, channel_id: usize, addr: usize, size: usize);
    fn read_at(&mut self, inode: u64, offset: usize, buf: &mut [u8]) -> usize;
    fn write_at(&mut self, inode: u64, offset: usize, data: &[u8]) -> usize;
    fn open(&mut self, path: &str, flags: u32) -> Result<(usize, usize), i32>;
    fn close(&mut self, inode: usize);
    fn mkdir(&self, path: &str);
    fn unlink(&self, path: &str);
    fn stat(&mut self, inode: usize) -> Stat;
    fn getdents64(&mut self, inode: u64, offset: usize, buf: &mut [u8]) -> (usize, usize);
}

/// Filesystem events
#[repr(usize)]
#[derive(Debug, Clone, Copy)]
pub enum FSIfaceEvent {
    init = 0,
    read_at = 1,
    write_at = 2,
    open = 3,
    mkdir = 4,
    unlink = 5,
    close = 6,
    stat = 7,
    getdents64 = 8,
}

impl TryFrom<usize> for FSIfaceEvent {
    type Error = ();
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::init),
            1 => Ok(Self::read_at),
            2 => Ok(Self::write_at),
            3 => Ok(Self::open),
            4 => Ok(Self::mkdir),
            5 => Ok(Self::unlink),
            6 => Ok(Self::close),
            7 => Ok(Self::stat),
            8 => Ok(Self::getdents64),
            _ => Err(()),
        }
    }
}

/// Stat structure
#[derive(Default, Clone, Copy)]
pub struct Stat {
    pub ino: usize,
    pub size: u64,
    pub mode: u32,
    pub nlink: u32,
    pub blksize: u32,
}
