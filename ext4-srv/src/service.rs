//! ext4 filesystem service implementation

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

use lwext4_task::{EXT4FSImpl, FSIface, Stat};
use blk_task::BLK;

use crate::ipc::{FsRequest, FsResponse, MAX_PATH_LEN};

/// File descriptor entry
struct FileEntry {
    path: String,
    inode: usize,
    size: usize,
    offset: usize,
    flags: u32,
}

/// Filesystem service state
pub struct FsService {
    fs: Mutex<EXT4FSImpl>,
    fd_table: Mutex<Vec<Option<FileEntry>>>,
    next_fd: Mutex<usize>,
}

impl FsService {
    /// Create a new filesystem service
    pub fn new() -> Self {
        // Initialize the block device with ext4 image
        let ext4_img = include_bytes!("../../http-boot/ext4.img");
        BLK.init_from_image(ext4_img);

        let fs = EXT4FSImpl::new();
        let mut fd_table = Vec::new();

        // Reserve FDs 0-2 for stdin/stdout/stderr
        fd_table.push(None); // 0: stdin
        fd_table.push(None); // 1: stdout
        fd_table.push(None); // 2: stderr

        FsService {
            fs: Mutex::new(fs),
            fd_table: Mutex::new(fd_table),
            next_fd: Mutex::new(3),
        }
    }

    /// Handle a filesystem request
    pub fn handle_request(&self, request: &FsRequest) -> FsResponse {
        match request {
            FsRequest::Open { path_addr, path_len, flags } => {
                self.handle_open(*path_addr, *path_len, *flags)
            }
            FsRequest::Read { fd, buf_addr, count } => {
                self.handle_read(*fd, *buf_addr, *count)
            }
            FsRequest::Write { fd, data_addr, count } => {
                self.handle_write(*fd, *data_addr, *count)
            }
            FsRequest::Close { fd } => {
                self.handle_close(*fd)
            }
            FsRequest::Stat { path_addr, path_len, stat_addr } => {
                self.handle_stat(*path_addr, *path_len, *stat_addr)
            }
            FsRequest::Getdents64 { fd, buf_addr, count } => {
                self.handle_getdents64(*fd, *buf_addr, *count)
            }
            FsRequest::Mkdir { path_addr, path_len } => {
                self.handle_mkdir(*path_addr, *path_len)
            }
            FsRequest::Unlink { path_addr, path_len } => {
                self.handle_unlink(*path_addr, *path_len)
            }
            FsRequest::Access { path_addr, path_len, mode } => {
                self.handle_access(*path_addr, *path_len, *mode)
            }
            FsRequest::FileSize { fd } => {
                self.handle_file_size(*fd)
            }
        }
    }

    /// Read a path from client memory (placeholder - in real impl would read from IPC buffer)
    fn read_path(&self, path_addr: usize, path_len: usize) -> String {
        // In real implementation, would read from client's memory via IPC
        // For now, return empty string
        String::new()
    }

    /// Handle open request
    fn handle_open(&self, path_addr: usize, path_len: usize, flags: u32) -> FsResponse {
        let path = self.read_path(path_addr, path_len);
        if path.is_empty() {
            return FsResponse::Err(-22); // EINVAL
        }

        let mut fs = self.fs.lock();
        match fs.open(&path, flags) {
            Ok((inode, size)) => {
                let mut fd_table = self.fd_table.lock();
                let mut next_fd = self.next_fd.lock();
                let fd = *next_fd;
                *next_fd += 1;

                // Ensure fd_table is large enough
                while fd_table.len() <= fd {
                    fd_table.push(None);
                }

                fd_table[fd] = Some(FileEntry {
                    path,
                    inode,
                    size,
                    offset: 0,
                    flags,
                });

                FsResponse::Ok(fd)
            }
            Err(e) => FsResponse::Err(e),
        }
    }

    /// Handle read request
    fn handle_read(&self, fd: usize, buf_addr: usize, count: usize) -> FsResponse {
        // Handle stdin
        if fd == 0 {
            return FsResponse::Ok(0); // EOF for stdin
        }

        // Handle stdout/stderr
        if fd == 1 || fd == 2 {
            return FsResponse::Err(-9); // EBADF
        }

        let fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get(fd) {
            let mut fs = self.fs.lock();
            let mut buf = vec![0u8; count];
            let bytes_read = fs.read_at(entry.inode as u64, entry.offset, &mut buf);

            // In real implementation, would write to client's memory via IPC
            // For now, just return bytes_read

            FsResponse::Ok(bytes_read)
        } else {
            FsResponse::Err(-9) // EBADF
        }
    }

    /// Handle write request
    fn handle_write(&self, fd: usize, data_addr: usize, count: usize) -> FsResponse {
        // Handle stdin
        if fd == 0 {
            return FsResponse::Err(-9); // EBADF
        }

        // Handle stdout/stderr - write to debug console
        if fd == 1 || fd == 2 {
            // In real implementation, would read from client's memory via IPC
            // For now, just return count
            return FsResponse::Ok(count);
        }

        let fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get(fd) {
            let mut fs = self.fs.lock();
            // In real implementation, would read data from client's memory
            let data = vec![0u8; count]; // Placeholder
            let bytes_written = fs.write_at(entry.inode as u64, entry.offset, &data);

            FsResponse::Ok(bytes_written)
        } else {
            FsResponse::Err(-9) // EBADF
        }
    }

    /// Handle close request
    fn handle_close(&self, fd: usize) -> FsResponse {
        // Handle stdin/stdout/stderr
        if fd <= 2 {
            return FsResponse::Ok(0);
        }

        let mut fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get(fd) {
            let mut fs = self.fs.lock();
            fs.close(entry.inode);
            fd_table[fd] = None;
            FsResponse::Ok(0)
        } else {
            FsResponse::Err(-9) // EBADF
        }
    }

    /// Handle stat request
    fn handle_stat(&self, path_addr: usize, path_len: usize, stat_addr: usize) -> FsResponse {
        let path = self.read_path(path_addr, path_len);
        if path.is_empty() {
            return FsResponse::Err(-22); // EINVAL
        }

        let mut fs = self.fs.lock();
        match fs.open(&path, 0) {
            Ok((inode, _)) => {
                let stat = fs.stat(inode);
                fs.close(inode);

                // In real implementation, would write stat to client's memory
                // For now, just return success
                FsResponse::Ok(0)
            }
            Err(e) => FsResponse::Err(e),
        }
    }

    /// Handle getdents64 request
    fn handle_getdents64(&self, fd: usize, buf_addr: usize, count: usize) -> FsResponse {
        let fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get(fd) {
            let mut fs = self.fs.lock();
            let mut buf = vec![0u8; count];
            let (bytes_read, new_offset) = fs.getdents64(entry.inode as u64, entry.offset, &mut buf);

            // In real implementation, would write to client's memory
            // For now, return (bytes_read, new_offset)
            FsResponse::Ok2(bytes_read, new_offset)
        } else {
            FsResponse::Err(-9) // EBADF
        }
    }

    /// Handle mkdir request
    fn handle_mkdir(&self, path_addr: usize, path_len: usize) -> FsResponse {
        let path = self.read_path(path_addr, path_len);
        if path.is_empty() {
            return FsResponse::Err(-22); // EINVAL
        }

        let fs = self.fs.lock();
        fs.mkdir(&path);
        FsResponse::Ok(0)
    }

    /// Handle unlink request
    fn handle_unlink(&self, path_addr: usize, path_len: usize) -> FsResponse {
        let path = self.read_path(path_addr, path_len);
        if path.is_empty() {
            return FsResponse::Err(-22); // EINVAL
        }

        let fs = self.fs.lock();
        fs.unlink(&path);
        FsResponse::Ok(0)
    }

    /// Handle access request
    fn handle_access(&self, path_addr: usize, path_len: usize, mode: u32) -> FsResponse {
        let path = self.read_path(path_addr, path_len);
        if path.is_empty() {
            return FsResponse::Err(-22); // EINVAL
        }

        let mut fs = self.fs.lock();
        match fs.open(&path, 0) {
            Ok((inode, _)) => {
                fs.close(inode);
                FsResponse::Ok(0) // Accessible
            }
            Err(_) => FsResponse::Err(-2), // ENOENT
        }
    }

    /// Handle file size request
    fn handle_file_size(&self, fd: usize) -> FsResponse {
        let fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get(fd) {
            FsResponse::Ok(entry.size)
        } else {
            FsResponse::Err(-9) // EBADF
        }
    }
}
