#![no_std]

extern crate alloc;

mod imp;

use alloc::string::String;
use core::iter::zip;
use core::mem::size_of;

use flatten_objects::FlattenObjects;
use imp::Ext4Disk;
use lwext4_rust::{
    Ext4BlockWrapper, Ext4File, InodeTypes,
    bindings::{O_CREAT, O_TRUNC},
};

const O_DIRECTORY: u32 = 0o40000;
const STORE_CAP: usize = 500;

/// 简化的文件系统接口 trait
pub trait FSIface {
    fn open(&mut self, path: &str, flags: u32) -> Result<(usize, usize), i32>;
    fn read_at(&mut self, inode: u64, offset: usize, buf: &mut [u8]) -> usize;
    fn write_at(&mut self, inode: u64, offset: usize, data: &[u8]) -> usize;
    fn close(&mut self, inode: usize);
    fn mkdir(&self, path: &str);
    fn unlink(&self, path: &str);
    fn stat(&mut self, inode: usize) -> Stat;
    fn getdents64(&mut self, inode: u64, offset: usize, buf: &mut [u8]) -> (usize, usize);
}

/// 简化的 Stat 结构
#[derive(Default, Clone, Copy)]
pub struct Stat {
    pub ino: usize,
    pub size: u64,
    pub mode: u32,
    pub nlink: u32,
    pub blksize: u32,
}

/// 简化的 Dirent64 结构
#[repr(C)]
pub struct Dirent64 {
    pub ino: u64,
    pub off: u64,
    pub reclen: u16,
    pub ftype: u8,
    pub name: [u8; 256],
}

/// EXT4 文件系统实现
pub struct EXT4FSImpl {
    _fs: Ext4BlockWrapper<Ext4Disk>,
    stores: FlattenObjects<Ext4File, STORE_CAP>,
}

unsafe impl Sync for EXT4FSImpl {}
unsafe impl Send for EXT4FSImpl {}

impl EXT4FSImpl {
    pub fn new() -> Self {
        EXT4FSImpl {
            _fs: Ext4BlockWrapper::new(Ext4Disk::new()).expect("Failed to create Ext4BlockWrapper"),
            stores: FlattenObjects::new(),
        }
    }
}

impl FSIface for EXT4FSImpl {
    fn open(&mut self, path: &str, flags: u32) -> Result<(usize, usize), i32> {
        let mut ext4_file = Ext4File::new("/", InodeTypes::EXT4_DE_DIR);

        if flags & O_CREAT == O_CREAT {
            if flags & O_DIRECTORY != O_DIRECTORY {
                ext4_file = Ext4File::new(path, InodeTypes::EXT4_DE_REG_FILE);
                ext4_file.file_open(path, flags | O_TRUNC).map_err(|e| e as i32)?;
            } else {
                return Err(-1);
            }
        } else if ext4_file.check_inode_exist(path, InodeTypes::EXT4_DE_DIR) {
            ext4_file = Ext4File::new(path, InodeTypes::EXT4_DE_DIR);
        } else if ext4_file.check_inode_exist(path, InodeTypes::EXT4_DE_REG_FILE) {
            ext4_file = Ext4File::new(path, InodeTypes::EXT4_DE_REG_FILE);
            ext4_file.file_open(path, flags).map_err(|e| e as i32)?;
        } else {
            return Err(-13); // EACCES
        }

        let file_size = ext4_file.file_size();
        if let Ok(index) = self.stores.add(ext4_file) {
            Ok((index, file_size as usize))
        } else {
            Err(-1)
        }
    }

    fn read_at(&mut self, inode: u64, offset: usize, buf: &mut [u8]) -> usize {
        if let Some(ext4_file) = self.stores.get_mut(inode as usize) {
            ext4_file.file_seek(offset as i64, 0).unwrap();
            ext4_file.file_read(buf).unwrap()
        } else {
            0
        }
    }

    fn write_at(&mut self, inode: u64, offset: usize, data: &[u8]) -> usize {
        if let Some(ext4_file) = self.stores.get_mut(inode as usize) {
            ext4_file.file_seek(offset as i64, 0).unwrap();
            ext4_file.file_write(data).unwrap()
        } else {
            0
        }
    }

    fn close(&mut self, inode: usize) {
        if let Some(mut ext4_file) = self.stores.remove(inode) {
            ext4_file.file_close().unwrap();
        }
    }

    fn mkdir(&self, path: &str) {
        let mut ext4_file = Ext4File::new(path, InodeTypes::EXT4_DE_DIR);
        ext4_file.dir_mk(path).unwrap();
    }

    fn unlink(&self, path: &str) {
        let mut ext4_file = Ext4File::new(path, InodeTypes::EXT4_DE_DIR);
        ext4_file.file_remove(path).unwrap();
    }

    fn stat(&mut self, inode: usize) -> Stat {
        if let Some(ext4_file) = self.stores.get_mut(inode) {
            Stat {
                ino: inode,
                size: ext4_file.file_size(),
                mode: ext4_file.file_mode_get().unwrap_or(0),
                nlink: 1,
                blksize: 0x200,
            }
        } else {
            Stat::default()
        }
    }

    fn getdents64(&mut self, inode: u64, mut offset: usize, buf: &mut [u8]) -> (usize, usize) {
        if let Some(ext4_file) = self.stores.get_mut(inode as usize) {
            let entries = ext4_file.lwext4_dir_entries().unwrap();
            let mut real_rlen: usize = 0;
            let mut base_ptr = buf.as_ptr() as usize;

            for (name, _ty) in zip(entries.0, entries.1).skip(offset) {
                let len = name.len() + size_of::<Dirent64>();
                let aligned = len.div_ceil(8) * 8;
                if real_rlen + aligned > buf.len() {
                    break;
                }
                let dirent = unsafe { &mut *(base_ptr as *mut Dirent64) };
                dirent.ftype = 0;
                dirent.reclen = aligned as u16;
                dirent.ino = 0;
                dirent.off = (real_rlen + aligned) as u64;
                let name_len = name.len().min(255);
                dirent.name[..name_len].copy_from_slice(&name[..name_len]);
                real_rlen += aligned;
                base_ptr += aligned;
                offset += 1;
            }
            (real_rlen, offset)
        } else {
            (0, offset)
        }
    }
}
