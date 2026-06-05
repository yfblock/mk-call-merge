//! IPC client for ext4 filesystem service
//!
//! This module provides a client interface to communicate with the ext4-srv
//! service via seL4 IPC.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

use sel4_sys::*;
use crate::syscall::SysResult;

/// Result type for void operations
type VoidResult = SysResult;

/// Result type for pair operations
type PairResult = SysResult;

/// IPC endpoint capability slot for the filesystem service
const FS_ENDPOINT_SLOT: usize = 100;

/// Request types
const REQ_OPEN: usize = 1;
const REQ_READ: usize = 2;
const REQ_WRITE: usize = 3;
const REQ_CLOSE: usize = 4;
const REQ_STAT: usize = 5;
const REQ_GETDENTS64: usize = 6;
const REQ_MKDIR: usize = 7;
const REQ_UNLINK: usize = 8;
const REQ_ACCESS: usize = 9;
const REQ_FILE_SIZE: usize = 10;

/// Response status
const RESP_OK: usize = 0;
const RESP_ERR: usize = 1;

/// File descriptor table entry
struct FdEntry {
    remote_fd: usize,
    path: String,
}

/// IPC client for filesystem service
pub struct FsClient {
    /// Local fd to remote fd mapping
    fd_table: Mutex<Vec<Option<FdEntry>>>,
    /// Next local fd
    next_fd: Mutex<usize>,
}

impl FsClient {
    /// Create a new filesystem client
    pub fn new() -> Self {
        let mut fd_table = Vec::new();

        // Reserve FDs 0-2 for stdin/stdout/stderr
        fd_table.push(None); // 0: stdin
        fd_table.push(None); // 1: stdout
        fd_table.push(None); // 2: stderr

        FsClient {
            fd_table: Mutex::new(fd_table),
            next_fd: Mutex::new(3),
        }
    }

    /// Send a request and receive response
    fn send_request(&self, req_type: usize, arg1: usize, arg2: usize, arg3: usize, arg4: usize) -> (usize, usize, usize) {
        with_ipc_buffer(|ib| {
            ib.write_mr(0, req_type);
            ib.write_mr(1, arg1);
            ib.write_mr(2, arg2);
            ib.write_mr(3, arg3);
            ib.write_mr(4, arg4);
        });

        let info = MessageInfo::new(0, 0, 0);
        let (tag, _badge) = seL4_Call(FS_ENDPOINT_SLOT, info.word());

        let msg = MessageInfo::from_word(tag);
        with_ipc_buffer(|ib| {
            let status = ib.read_mr(0);
            let r1 = ib.read_mr(1);
            let r2 = ib.read_mr(2);
            (status, r1, r2)
        })
    }

    /// Open a file
    pub fn open(&self, path: &str, flags: u32) -> SysResult {
        // In real implementation, would pass path via shared memory
        // For now, use a placeholder
        let path_addr = 0; // Placeholder
        let path_len = path.len();

        let (status, result, _) = self.send_request(REQ_OPEN, path_addr, path_len, flags as usize, 0);

        if status == RESP_OK {
            let mut fd_table = self.fd_table.lock();
            let mut next_fd = self.next_fd.lock();
            let local_fd = *next_fd;
            *next_fd += 1;

            while fd_table.len() <= local_fd {
                fd_table.push(None);
            }

            fd_table[local_fd] = Some(FdEntry {
                remote_fd: result,
                path: String::from(path),
            });

            Ok(local_fd)
        } else {
            Err(result as i32)
        }
    }

    /// Read from file
    pub fn read(&self, fd: usize, buf: &mut [u8]) -> SysResult {
        // Handle stdin
        if fd == 0 {
            return Ok(0); // EOF
        }

        // Handle stdout/stderr
        if fd == 1 || fd == 2 {
            return Err(-9); // EBADF
        }

        let fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get(fd) {
            let (status, result, _) = self.send_request(
                REQ_READ,
                entry.remote_fd,
                0, // buf_addr placeholder
                buf.len(),
                0,
            );

            if status == RESP_OK {
                // In real implementation, would copy data from shared memory
                Ok(result)
            } else {
                Err(result as i32)
            }
        } else {
            Err(-9) // EBADF
        }
    }

    /// Write to file
    pub fn write(&self, fd: usize, data: &[u8]) -> SysResult {
        // Handle stdin
        if fd == 0 {
            return Err(-9); // EBADF
        }

        // Handle stdout/stderr - write to debug console
        if fd == 1 || fd == 2 {
            for &byte in data {
                seL4_DebugPutChar(byte);
            }
            return Ok(data.len());
        }

        let fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get(fd) {
            let (status, result, _) = self.send_request(
                REQ_WRITE,
                entry.remote_fd,
                0, // data_addr placeholder
                data.len(),
                0,
            );

            if status == RESP_OK {
                Ok(result)
            } else {
                Err(result as i32)
            }
        } else {
            Err(-9) // EBADF
        }
    }

    /// Close file
    pub fn close(&self, fd: usize) -> VoidResult {
        // Handle stdin/stdout/stderr
        if fd <= 2 {
            return Ok(0);
        }

        let mut fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get(fd) {
            let (status, result, _) = self.send_request(REQ_CLOSE, entry.remote_fd, 0, 0, 0);

            if status == RESP_OK {
                fd_table[fd] = None;
                Ok(0)
            } else {
                Err(result as i32)
            }
        } else {
            Err(-9) // EBADF
        }
    }

    /// Get file status
    pub fn stat(&self, path: &str) -> VoidResult {
        let path_addr = 0; // Placeholder
        let path_len = path.len();

        let (status, result, _) = self.send_request(REQ_STAT, path_addr, path_len, 0, 0);

        if status == RESP_OK {
            Ok(0)
        } else {
            Err(result as i32)
        }
    }

    /// Read directory entries
    pub fn getdents64(&self, fd: usize, buf: &mut [u8]) -> PairResult {
        let fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get(fd) {
            let (status, r1, r2) = self.send_request(
                REQ_GETDENTS64,
                entry.remote_fd,
                0, // buf_addr placeholder
                buf.len(),
                0,
            );

            if status == RESP_OK {
                Ok(r1)
            } else {
                Err(r1 as i32)
            }
        } else {
            Err(-9) // EBADF
        }
    }

    /// Create directory
    pub fn mkdir(&self, path: &str) -> VoidResult {
        let path_addr = 0; // Placeholder
        let path_len = path.len();

        let (status, result, _) = self.send_request(REQ_MKDIR, path_addr, path_len, 0, 0);

        if status == RESP_OK {
            Ok(0)
        } else {
            Err(result as i32)
        }
    }

    /// Remove file
    pub fn unlink(&self, path: &str) -> VoidResult {
        let path_addr = 0; // Placeholder
        let path_len = path.len();

        let (status, result, _) = self.send_request(REQ_UNLINK, path_addr, path_len, 0, 0);

        if status == RESP_OK {
            Ok(0)
        } else {
            Err(result as i32)
        }
    }

    /// Check file access
    pub fn access(&self, path: &str, mode: u32) -> VoidResult {
        let path_addr = 0; // Placeholder
        let path_len = path.len();

        let (status, result, _) = self.send_request(REQ_ACCESS, path_addr, path_len, mode as usize, 0);

        if status == RESP_OK {
            Ok(0)
        } else {
            Err(result as i32)
        }
    }

    /// Get file size
    pub fn file_size(&self, fd: usize) -> SysResult {
        let fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get(fd) {
            let (status, result, _) = self.send_request(REQ_FILE_SIZE, entry.remote_fd, 0, 0, 0);

            if status == RESP_OK {
                Ok(result)
            } else {
                Err(result as i32)
            }
        } else {
            Err(-9) // EBADF
        }
    }
}

/// Global filesystem client instance
pub static FS_CLIENT: spin::Lazy<FsClient> = spin::Lazy::new(|| FsClient::new());
