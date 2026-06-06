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

    /// Handle a filesystem request (legacy path — used for fd-only ops)
    pub fn handle_request(&self, request: &FsRequest) -> FsResponse {
        match request {
            FsRequest::Read { fd, count, .. } => {
                self.handle_read(*fd, *count)
            }
            FsRequest::Close { fd } => {
                self.handle_close(*fd)
            }
            FsRequest::Getdents64 { fd, count, .. } => {
                self.handle_getdents64(*fd, *count)
            }
            FsRequest::FileSize { fd } => {
                self.handle_file_size(*fd)
            }
            _ => FsResponse::Err(-38), // ENOSYS for unhandled variants
        }
    }

    // ── path-bearing handlers (path already decoded from MRs) ──────

    /// Open a file/directory using a pre-decoded path
    pub fn handle_open_with_path(&self, path: &str, flags: u32) -> FsResponse {
        if path.is_empty() {
            return FsResponse::Err(-22); // EINVAL
        }

        let mut fs = self.fs.lock();
        match fs.open(path, flags) {
            Ok((inode, size)) => {
                let mut fd_table = self.fd_table.lock();
                let mut next_fd = self.next_fd.lock();
                let fd = *next_fd;
                *next_fd += 1;

                while fd_table.len() <= fd {
                    fd_table.push(None);
                }

                fd_table[fd] = Some(FileEntry {
                    path: String::from(path),
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

    /// Stat using a pre-decoded path — returns (mode, size_lo, size_hi, ino, nlink)
    pub fn handle_stat_with_path(&self, path: &str) -> FsResponse {
        if path.is_empty() {
            return FsResponse::Err(-22);
        }

        let mut fs = self.fs.lock();
        match fs.open(path, 0) {
            Ok((inode, _)) => {
                let stat = fs.stat(inode);
                fs.close(inode);

                // Return 5 values packed into Ok/Ok2 + extra MRs.
                // Use Ok2 for the first two, and the caller reads MR3..MR5
                // from the IPC buffer.  But since our IPC helper only handles
                // Ok / Ok2 / Err, we pack into the generic "Ok" variant with
                // mode in the result and rely on the caller reading more MRs.
                //
                // Simpler: use Ok2(mode, size_lo) and let the main loop also
                // write size_hi/ino/nlink into MR3..5 before replying.
                // → We need a new variant or a custom approach.
                //
                // Simplest: just write all 5 fields into a small Vec and use
                // OkWithData.  The client can unpack.
                let mut data = Vec::with_capacity(20);
                data.extend_from_slice(&(stat.mode as u32).to_le_bytes());
                data.extend_from_slice(&(stat.size as u32).to_le_bytes());
                data.extend_from_slice(((stat.size >> 32) as u32).to_le_bytes().as_ref());
                data.extend_from_slice(&(stat.ino as u32).to_le_bytes());
                data.extend_from_slice(&(stat.nlink as u32).to_le_bytes());
                FsResponse::OkWithData(data)
            }
            Err(e) => FsResponse::Err(e),
        }
    }

    /// Mkdir using a pre-decoded path
    pub fn handle_mkdir_with_path(&self, path: &str) -> FsResponse {
        if path.is_empty() {
            return FsResponse::Err(-22);
        }
        let fs = self.fs.lock();
        fs.mkdir(path);
        FsResponse::Ok(0)
    }

    /// Unlink using a pre-decoded path
    pub fn handle_unlink_with_path(&self, path: &str) -> FsResponse {
        if path.is_empty() {
            return FsResponse::Err(-22);
        }
        let fs = self.fs.lock();
        fs.unlink(path);
        FsResponse::Ok(0)
    }

    /// Access check using a pre-decoded path
    pub fn handle_access_with_path(&self, path: &str) -> FsResponse {
        if path.is_empty() {
            return FsResponse::Err(-22);
        }
        let mut fs = self.fs.lock();
        match fs.open(path, 0) {
            Ok((inode, _)) => {
                fs.close(inode);
                FsResponse::Ok(0)
            }
            Err(_) => FsResponse::Err(-2), // ENOENT
        }
    }

    /// Write with data already decoded from IPC
    pub fn handle_write_with_data(&self, fd: usize, data: &[u8]) -> FsResponse {
        if fd == 0 { return FsResponse::Err(-9); }
        if fd == 1 || fd == 2 {
            for &b in data {
                sel4_sys::seL4_DebugPutChar(b);
            }
            return FsResponse::Ok(data.len());
        }

        let fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get(fd) {
            let mut fs = self.fs.lock();
            let bytes_written = fs.write_at(entry.inode as u64, entry.offset, data);
            FsResponse::Ok(bytes_written)
        } else {
            FsResponse::Err(-9)
        }
    }

    /// Lseek — update the offset of an open fd
    pub fn handle_lseek(&self, fd: usize, offset: isize, whence: i32) -> FsResponse {
        if fd <= 2 { return FsResponse::Err(-29); } // ESPIPE

        let mut fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get_mut(fd) {
            // Linux SEEK_SET=0, SEEK_CUR=1, SEEK_END=2
            let new_off = match whence {
                0 => {
                    if offset < 0 { return FsResponse::Err(-22); } // EINVAL
                    offset as usize
                }
                1 => (entry.offset as isize + offset) as usize,
                2 => {
                    if offset > 0 { return FsResponse::Err(-22); }
                    (entry.size as isize + offset) as usize
                }
                _ => return FsResponse::Err(-22),
            };
            entry.offset = new_off;
            FsResponse::Ok(new_off)
        } else {
            FsResponse::Err(-9) // EBADF
        }
    }

    // ── fd-based handlers (no path needed) ─────────────────────────

    /// Handle read request — returns data inline via OkWithData
    fn handle_read(&self, fd: usize, count: usize) -> FsResponse {
        if fd == 0 { return FsResponse::Ok(0); } // stdin EOF
        if fd == 1 || fd == 2 { return FsResponse::Err(-9); }

        let mut fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get_mut(fd) {
            let mut fs = self.fs.lock();
            let mut buf = vec![0u8; count];
            let bytes_read = fs.read_at(entry.inode as u64, entry.offset, &mut buf);
            entry.offset += bytes_read;
            buf.truncate(bytes_read);
            FsResponse::OkWithData(buf)
        } else {
            FsResponse::Err(-9)
        }
    }

    /// Handle close request
    fn handle_close(&self, fd: usize) -> FsResponse {
        if fd <= 2 { return FsResponse::Ok(0); }

        let mut fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get(fd) {
            let mut fs = self.fs.lock();
            fs.close(entry.inode);
            fd_table[fd] = None;
            FsResponse::Ok(0)
        } else {
            FsResponse::Err(-9)
        }
    }

    /// Handle getdents64 — returns directory entries inline via OkWithData2
    fn handle_getdents64(&self, fd: usize, count: usize) -> FsResponse {
        let mut fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get_mut(fd) {
            let mut fs = self.fs.lock();
            let mut buf = vec![0u8; count];
            let (bytes_read, new_offset) = fs.getdents64(entry.inode as u64, entry.offset, &mut buf);

            // Update the stored offset so the next call continues where we left off
            entry.offset = new_offset;

            buf.truncate(bytes_read);
            FsResponse::OkWithData2(buf, new_offset)
        } else {
            FsResponse::Err(-9)
        }
    }

    /// Handle file size request
    fn handle_file_size(&self, fd: usize) -> FsResponse {
        let fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get(fd) {
            FsResponse::Ok(entry.size)
        } else {
            FsResponse::Err(-9)
        }
    }
}
