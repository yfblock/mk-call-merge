//! IPC client for ext4 filesystem service
//!
//! This module provides a client interface to communicate with the ext4-srv
//! service via seL4 IPC.
//!
//! ## IPC Protocol
//!
//! **Request** (client → server):
//!   MR0 = req_type, MR1..MR4 = args, MR5+ = path bytes (if applicable)
//!   `MessageInfo.length` is set so the kernel copies all used MRs.
//!
//! **Response** (server → client):
//!   MR0 = status (0=ok, 1=err), MR1 = result / errno
//!   For data-returning operations: MR2+ = payload bytes

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

use sel4_sys::{IpcBuffer, MessageInfo, seL4_Call, seL4_DebugPutChar, with_ipc_buffer};
use crate::syscall::SysResult;

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
const REQ_LSEEK: usize = 11;

/// Response status
const RESP_OK: usize = 0;
const RESP_ERR: usize = 1;

/// MR index where path/data payload starts
const PATH_MR_START: usize = 5;
const DATA_MR_START: usize = 2; // response: MR0=status, MR1=result, MR2+=data

/// Maximum path bytes that fit in MRs: (120 - PATH_MR_START) * 8
const MAX_PATH_IPC: usize = (120 - PATH_MR_START) * 8; // 920 bytes

/// Maximum data bytes that fit in response MRs: (120 - DATA_MR_START) * 8
const MAX_DATA_IPC: usize = (120 - DATA_MR_START) * 8; // 944 bytes

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

// ─── helpers ────────────────────────────────────────────────────────

/// Encode raw bytes into IPC message registers starting at `start_mr`.
/// Returns the number of MRs consumed.
fn encode_bytes_to_mrs(ib: &mut IpcBuffer, data: &[u8], start_mr: usize) -> usize {
    let num_mrs = (data.len() + 7) / 8;
    for i in 0..num_mrs {
        let mut word: usize = 0;
        for j in 0..8 {
            let idx = i * 8 + j;
            if idx < data.len() {
                word |= (data[idx] as usize) << (j * 8);
            }
        }
        ib.write_mr(start_mr + i, word);
    }
    num_mrs
}

/// Decode `len` raw bytes from IPC message registers starting at `start_mr`.
fn decode_bytes_from_mrs(ib: &IpcBuffer, start_mr: usize, len: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(len);
    let num_mrs = (len + 7) / 8;
    for i in 0..num_mrs {
        let word = ib.read_mr(start_mr + i);
        for j in 0..8 {
            if i * 8 + j < len {
                bytes.push(((word >> (j * 8)) & 0xFF) as u8);
            }
        }
    }
    bytes
}

// ─── impl ───────────────────────────────────────────────────────────

impl FsClient {
    /// Create a new filesystem client
    pub fn new() -> Self {
        let mut fd_table = Vec::new();
        fd_table.push(None); // 0: stdin
        fd_table.push(None); // 1: stdout
        fd_table.push(None); // 2: stderr

        FsClient {
            fd_table: Mutex::new(fd_table),
            next_fd: Mutex::new(3),
        }
    }

    /// Low-level IPC round-trip.
    ///
    /// The caller must have already written MR0..`total_mrs-1` into the IPC
    /// buffer.  This function sets the `MessageInfo.length` so the kernel
    /// copies exactly those MRs, calls `seL4_Call`, and returns
    /// `(MR0, MR1, MR2)` from the server's reply.
    fn do_call(&self, total_mrs: usize) -> (usize, usize, usize) {
        let info_word = total_mrs & 0x7F; // length field only
        let (_err, _tag) = seL4_Call(FS_ENDPOINT_SLOT, info_word);

        with_ipc_buffer(|ib| {
            (ib.read_mr(0), ib.read_mr(1), ib.read_mr(2))
        })
    }

    // ── path-bearing requests ───────────────────────────────────────

    /// Open a file / directory
    pub fn open(&self, path: &str, flags: u32) -> SysResult {
        let path_bytes = path.as_bytes();
        if path_bytes.len() > MAX_PATH_IPC {
            return Err(-36); // ENAMETOOLONG
        }
        let path_mrs = (path_bytes.len() + 7) / 8;
        let total_mrs = PATH_MR_START + path_mrs;

        with_ipc_buffer(|ib| {
            ib.write_mr(0, REQ_OPEN);
            ib.write_mr(1, 0); // unused (was path_addr)
            ib.write_mr(2, path_bytes.len());
            ib.write_mr(3, flags as usize);
            ib.write_mr(4, 0);
            encode_bytes_to_mrs(ib, path_bytes, PATH_MR_START);
        });

        let (status, result, _) = self.do_call(total_mrs);

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

    /// Get file status — returns (mode, size, ino, nlink) from the server
    pub fn stat(&self, path: &str) -> Result<(u32, u64, u64, u32), i32> {
        let path_bytes = path.as_bytes();
        if path_bytes.len() > MAX_PATH_IPC {
            return Err(-36);
        }
        let path_mrs = (path_bytes.len() + 7) / 8;
        let total_mrs = PATH_MR_START + path_mrs;

        with_ipc_buffer(|ib| {
            ib.write_mr(0, REQ_STAT);
            ib.write_mr(1, 0);
            ib.write_mr(2, path_bytes.len());
            ib.write_mr(3, 0);
            ib.write_mr(4, 0);
            encode_bytes_to_mrs(ib, path_bytes, PATH_MR_START);
        });

        let (status, r1, r2) = self.do_call(total_mrs);

        if status == RESP_OK {
            // Server returns: MR1=mode, MR2=size_lo, MR3=size_hi, MR4=ino, MR5=nlink
            let (size_hi, ino, nlink) = with_ipc_buffer(|ib| {
                (ib.read_mr(3), ib.read_mr(4), ib.read_mr(5))
            });
            let size = ((size_hi as u64) << 32) | (r2 as u64);
            Ok((r1 as u32, size, ino as u64, nlink as u32))
        } else {
            Err(r1 as i32)
        }
    }

    // ── fd-based requests ───────────────────────────────────────────

    /// Read from file
    pub fn read(&self, fd: usize, buf: &mut [u8]) -> SysResult {
        if fd == 0 { return Ok(0); }
        if fd == 1 || fd == 2 { return Err(-9); }

        let fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get(fd) {
            let remote_fd = entry.remote_fd;
            drop(fd_table);

            with_ipc_buffer(|ib| {
                ib.write_mr(0, REQ_READ);
                ib.write_mr(1, remote_fd);
                ib.write_mr(2, 0);
                ib.write_mr(3, buf.len().min(MAX_DATA_IPC));
                ib.write_mr(4, 0);
            });

            let (status, r1, _r2) = self.do_call(5);

            if status == RESP_OK && r1 > 0 {
                let data = with_ipc_buffer(|ib| {
                    decode_bytes_from_mrs(ib, DATA_MR_START, r1)
                });
                let n = r1.min(buf.len()).min(data.len());
                buf[..n].copy_from_slice(&data[..n]);
                Ok(n)
            } else if status == RESP_OK {
                Ok(0) // EOF
            } else {
                Err(r1 as i32)
            }
        } else {
            Err(-9)
        }
    }

    /// Write to file (data encoded in MRs for small payloads)
    pub fn write(&self, fd: usize, data: &[u8]) -> SysResult {
        if fd == 0 { return Err(-9); }
        if fd == 1 || fd == 2 {
            for &b in data { seL4_DebugPutChar(b); }
            return Ok(data.len());
        }

        let fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get(fd) {
            let remote_fd = entry.remote_fd;
            drop(fd_table);

            let payload = data.len().min(MAX_DATA_IPC);
            with_ipc_buffer(|ib| {
                ib.write_mr(0, REQ_WRITE);
                ib.write_mr(1, remote_fd);
                ib.write_mr(2, 0);
                ib.write_mr(3, payload);
                ib.write_mr(4, 0);
                encode_bytes_to_mrs(ib, &data[..payload], DATA_MR_START);
            });

            let total_mrs = DATA_MR_START + (payload + 7) / 8;
            let (status, r1, _) = self.do_call(total_mrs);

            if status == RESP_OK { Ok(r1) } else { Err(r1 as i32) }
        } else {
            Err(-9)
        }
    }

    /// Close file
    pub fn close(&self, fd: usize) -> SysResult {
        if fd <= 2 { return Ok(0); }

        let mut fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get(fd) {
            let remote_fd = entry.remote_fd;
            drop(fd_table);

            with_ipc_buffer(|ib| {
                ib.write_mr(0, REQ_CLOSE);
                ib.write_mr(1, remote_fd);
                ib.write_mr(2, 0);
                ib.write_mr(3, 0);
                ib.write_mr(4, 0);
            });
            let (status, r1, _) = self.do_call(5);

            if status == RESP_OK {
                fd_table = self.fd_table.lock();
                fd_table[fd] = None;
                Ok(0)
            } else {
                Err(r1 as i32)
            }
        } else {
            Err(-9)
        }
    }

    /// Read directory entries
    pub fn getdents64(&self, fd: usize, buf: &mut [u8]) -> SysResult {
        let fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get(fd) {
            let remote_fd = entry.remote_fd;
            drop(fd_table);

            with_ipc_buffer(|ib| {
                ib.write_mr(0, REQ_GETDENTS64);
                ib.write_mr(1, remote_fd);
                ib.write_mr(2, 0);
                ib.write_mr(3, buf.len().min(MAX_DATA_IPC));
                ib.write_mr(4, 0);
            });

            let (status, r1, _r2) = self.do_call(5);

            if status == RESP_OK && r1 > 0 {
                let data = with_ipc_buffer(|ib| {
                    decode_bytes_from_mrs(ib, DATA_MR_START, r1)
                });
                let n = r1.min(buf.len()).min(data.len());
                buf[..n].copy_from_slice(&data[..n]);
                Ok(r1) // return total bytes_read from server
            } else if status == RESP_OK {
                Ok(0)
            } else {
                Err(r1 as i32)
            }
        } else {
            Err(-9)
        }
    }

    /// Seek in file
    pub fn lseek(&self, fd: usize, offset: isize, whence: i32) -> SysResult {
        let fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get(fd) {
            let remote_fd = entry.remote_fd;
            drop(fd_table);

            with_ipc_buffer(|ib| {
                ib.write_mr(0, REQ_LSEEK);
                ib.write_mr(1, remote_fd);
                ib.write_mr(2, offset as usize);
                ib.write_mr(3, whence as usize);
                ib.write_mr(4, 0);
            });
            let (status, r1, _) = self.do_call(5);
            if status == RESP_OK { Ok(r1) } else { Err(r1 as i32) }
        } else {
            Err(-9)
        }
    }

    // ── path-only requests (no data in response) ────────────────────

    /// Create directory
    pub fn mkdir(&self, path: &str) -> SysResult {
        let pb = path.as_bytes();
        if pb.len() > MAX_PATH_IPC { return Err(-36); }
        let path_mrs = (pb.len() + 7) / 8;

        with_ipc_buffer(|ib| {
            ib.write_mr(0, REQ_MKDIR);
            ib.write_mr(1, 0);
            ib.write_mr(2, pb.len());
            ib.write_mr(3, 0);
            ib.write_mr(4, 0);
            encode_bytes_to_mrs(ib, pb, PATH_MR_START);
        });
        let (s, r, _) = self.do_call(PATH_MR_START + path_mrs);
        if s == RESP_OK { Ok(0) } else { Err(r as i32) }
    }

    /// Remove file
    pub fn unlink(&self, path: &str) -> SysResult {
        let pb = path.as_bytes();
        if pb.len() > MAX_PATH_IPC { return Err(-36); }
        let path_mrs = (pb.len() + 7) / 8;

        with_ipc_buffer(|ib| {
            ib.write_mr(0, REQ_UNLINK);
            ib.write_mr(1, 0);
            ib.write_mr(2, pb.len());
            ib.write_mr(3, 0);
            ib.write_mr(4, 0);
            encode_bytes_to_mrs(ib, pb, PATH_MR_START);
        });
        let (s, r, _) = self.do_call(PATH_MR_START + path_mrs);
        if s == RESP_OK { Ok(0) } else { Err(r as i32) }
    }

    /// Check file access
    pub fn access(&self, path: &str, mode: u32) -> SysResult {
        let pb = path.as_bytes();
        if pb.len() > MAX_PATH_IPC { return Err(-36); }
        let path_mrs = (pb.len() + 7) / 8;

        with_ipc_buffer(|ib| {
            ib.write_mr(0, REQ_ACCESS);
            ib.write_mr(1, 0);
            ib.write_mr(2, pb.len());
            ib.write_mr(3, mode as usize);
            ib.write_mr(4, 0);
            encode_bytes_to_mrs(ib, pb, PATH_MR_START);
        });
        let (s, r, _) = self.do_call(PATH_MR_START + path_mrs);
        if s == RESP_OK { Ok(0) } else { Err(r as i32) }
    }

    /// Get file size
    pub fn file_size(&self, fd: usize) -> SysResult {
        let fd_table = self.fd_table.lock();
        if let Some(Some(entry)) = fd_table.get(fd) {
            let remote_fd = entry.remote_fd;
            drop(fd_table);

            with_ipc_buffer(|ib| {
                ib.write_mr(0, REQ_FILE_SIZE);
                ib.write_mr(1, remote_fd);
                ib.write_mr(2, 0);
                ib.write_mr(3, 0);
                ib.write_mr(4, 0);
            });
            let (s, r, _) = self.do_call(5);
            if s == RESP_OK { Ok(r) } else { Err(r as i32) }
        } else {
            Err(-9)
        }
    }
}

/// Global filesystem client instance
pub static FS_CLIENT: spin::Lazy<FsClient> = spin::Lazy::new(|| FsClient::new());
