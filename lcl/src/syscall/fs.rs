//! File system syscalls - ported from kernel-thread
//!
//! Implements Linux file operations for the LCL environment.

use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use common::config::PAGE_SIZE;
use crate::task::Sel4Task;
use crate::syscall::SysResult;
use crate::fs::ipc_client::FS_CLIENT;

/// Linux open flags
pub const O_RDONLY: u32 = 0;
pub const O_WRONLY: u32 = 1;
pub const O_RDWR: u32 = 2;
pub const O_CREAT: u32 = 64;
pub const O_TRUNC: u32 = 512;
pub const O_APPEND: u32 = 1024;
pub const O_DIRECTORY: u32 = 0x10000;

/// Linux AT_FDCWD
pub const AT_FDCWD: i32 = -100;

/// Read from file descriptor
pub fn sys_read(task: &Arc<Sel4Task>, fd: usize, buf_addr: usize, count: usize) -> SysResult {
    match fd {
        0 => {
            // stdin - return 0 (EOF)
            Ok(0)
        }
        1 | 2 => {
            // stdout/stderr - can't read from output
            Err(9) // EBADF
        }
        10 => {
            // /dev/null - return 0 (EOF)
            Ok(0)
        }
        11 => {
            // /dev/zero - fill with zeros
            let zeros = vec![0u8; count];
            if task.write_bytes(buf_addr, &zeros) {
                Ok(count)
            } else {
                Err(14) // EFAULT
            }
        }
        _ => {
            // Try to read via filesystem service
            let mut buf = vec![0u8; count];
            match FS_CLIENT.read(fd, &mut buf) {
                Ok(bytes_read) => {
                    if task.write_bytes(buf_addr, &buf[..bytes_read]) {
                        Ok(bytes_read)
                    } else {
                        Err(14) // EFAULT
                    }
                }
                Err(e) => Err(-e),
            }
        }
    }
}

/// Write to file descriptor
pub fn sys_write(task: &Arc<Sel4Task>, fd: usize, buf_addr: usize, count: usize) -> SysResult {
    match fd {
        0 => {
            // stdin - can't write to input
            Err(9) // EBADF
        }
        1 | 2 => {
            // stdout/stderr - write to serial console
            for i in 0..count {
                let mut byte = [0u8; 1];
                if task.read_bytes(buf_addr + i, &mut byte) {
                    sel4_sys::seL4_DebugPutChar(byte[0]);
                }
            }
            Ok(count)
        }
        10 => {
            // /dev/null - discard
            Ok(count)
        }
        11 => {
            // /dev/zero - discard
            Ok(count)
        }
        _ => {
            // Read data from task memory
            let mut data = vec![0u8; count];
            if !task.read_bytes(buf_addr, &mut data) {
                return Err(14); // EFAULT
            }

            // Try to write via filesystem service
            match FS_CLIENT.write(fd, &data) {
                Ok(bytes_written) => Ok(bytes_written),
                Err(e) => Err(-e),
            }
        }
    }
}

/// Open file
pub fn sys_openat(task: &Arc<Sel4Task>, dirfd: i32, path_addr: usize, flags: u32, _mode: u32) -> SysResult {
    // Read path from task memory
    let path = read_cstr_from_task(task, path_addr);

    // Handle device files
    match path.as_str() {
        "/dev/null" => Ok(10),
        "/dev/zero" => Ok(11),
        "/dev/urandom" => Ok(12),
        _ => {
            // Try to open via filesystem service
            match FS_CLIENT.open(&path, flags) {
                Ok(fd) => Ok(fd),
                Err(e) => Err(-e),
            }
        }
    }
}

/// Close file descriptor
pub fn sys_close(_task: &Arc<Sel4Task>, fd: usize) -> SysResult {
    match fd {
        0..=2 => Ok(0), // stdin/stdout/stderr - always OK
        10..=12 => Ok(0), // Device files - always OK
        _ => {
            // Try to close via filesystem service
            match FS_CLIENT.close(fd) {
                Ok(_) => Ok(0),
                Err(e) => Err(-e),
            }
        }
    }
}

/// Seek in file
pub fn sys_lseek(_task: &Arc<Sel4Task>, fd: usize, offset: isize, whence: i32) -> SysResult {
    match fd {
        0..=2 | 10..=12 => Err(29), // ESPIPE
        _ => match FS_CLIENT.lseek(fd, offset, whence) {
            Ok(pos) => Ok(pos),
            Err(e) => Err(-e),
        },
    }
}

/// Get file status
pub fn sys_fstat(_task: &Arc<Sel4Task>, fd: usize, _stat_addr: usize) -> SysResult {
    // Handle device files
    match fd {
        0..=2 | 10..=12 => Ok(0), // Device files - return minimal stat
        _ => {
            // Try to get file size via filesystem service
            match FS_CLIENT.file_size(fd) {
                Ok(_size) => {
                    // In real implementation, would write stat structure to stat_addr
                    Ok(0)
                }
                Err(e) => Err(-e),
            }
        }
    }
}

/// Get file status relative to directory.
///
/// On success, writes a Linux x86_64 `struct stat` (144 bytes) to `stat_addr`
/// in the child's address space.  Only the fields that `ls` actually reads are
/// filled; the rest are zeroed.
pub fn sys_fstatat(task: &Arc<Sel4Task>, _dirfd: i32, path_addr: usize, stat_addr: usize, _flags: u32) -> SysResult {
    let path = read_cstr_from_task(task, path_addr);

    match FS_CLIENT.stat(&path) {
        Ok((mode, size, ino, nlink)) => {
            // Build a minimal Linux x86_64 struct stat (144 bytes).
            // Layout: st_dev(8) st_ino(8) st_nlink(8) st_mode(4) st_uid(4)
            //         st_gid(4) __pad0(4) st_rdev(8) st_size(8) st_blksize(8)
            //         st_blocks(8) st_atim(16) st_mtim(16) st_ctim(16)
            let mut buf = [0u8; 144];
            // st_ino @ offset 8
            buf[8..16].copy_from_slice(&(ino as u64).to_le_bytes());
            // st_nlink @ offset 16
            buf[16..24].copy_from_slice(&(nlink as u64).to_le_bytes());
            // st_mode @ offset 24
            buf[24..28].copy_from_slice(&(mode as u32).to_le_bytes());
            // st_uid = 0, st_gid = 0  (already zero)
            // st_size @ offset 48
            buf[48..56].copy_from_slice(&(size as u64).to_le_bytes());
            // st_blksize @ offset 56
            buf[56..64].copy_from_slice(&4096u64.to_le_bytes());
            // st_blocks @ offset 64
            let blocks = (size + 511) / 512;
            buf[64..72].copy_from_slice(&(blocks as u64).to_le_bytes());

            if task.write_bytes(stat_addr, &buf) {
                Ok(0)
            } else {
                Err(14) // EFAULT
            }
        }
        Err(e) => Err(-e),
    }
}

/// Control file descriptor
pub fn sys_fcntl(_task: &Arc<Sel4Task>, _fd: usize, _cmd: usize, _arg: usize) -> SysResult {
    Ok(0)
}

/// I/O control
pub fn sys_ioctl(_task: &Arc<Sel4Task>, _fd: usize, _request: usize, _arg: usize) -> SysResult {
    Ok(0)
}

/// Create directory
pub fn sys_mkdirat(task: &Arc<Sel4Task>, _dirfd: i32, path_addr: usize, _mode: u32) -> SysResult {
    let path = read_cstr_from_task(task, path_addr);
    match FS_CLIENT.mkdir(&path) {
        Ok(_) => Ok(0),
        Err(e) => Err(-e),
    }
}

/// Remove directory
pub fn sys_unlinkat(task: &Arc<Sel4Task>, _dirfd: i32, path_addr: usize, _flags: u32) -> SysResult {
    let path = read_cstr_from_task(task, path_addr);
    match FS_CLIENT.unlink(&path) {
        Ok(_) => Ok(0),
        Err(e) => Err(-e),
    }
}

/// Rename
pub fn sys_renameat(_task: &Arc<Sel4Task>, _olddirfd: i32, _oldpath_addr: usize, _newdirfd: i32, _newpath_addr: usize) -> SysResult {
    Ok(0)
}

/// Truncate file
pub fn sys_ftruncate(_task: &Arc<Sel4Task>, _fd: usize, _length: isize) -> SysResult {
    Ok(0)
}

/// Read directory entries
pub fn sys_getdents64(task: &Arc<Sel4Task>, fd: usize, buf_addr: usize, count: usize) -> SysResult {
    let mut buf = vec![0u8; count];
    match FS_CLIENT.getdents64(fd, &mut buf) {
        Ok(bytes_read) => {
            if bytes_read > 0 {
                if task.write_bytes(buf_addr, &buf[..bytes_read]) {
                    Ok(bytes_read)
                } else {
                    Err(14) // EFAULT
                }
            } else {
                Ok(0) // No more entries
            }
        }
        Err(e) => Err(-e),
    }
}

/// Pipe
pub fn sys_pipe2(_task: &Arc<Sel4Task>, _pipefd_addr: usize, _flags: u32) -> SysResult {
    Err(38) // ENOSYS - not implemented
}

/// Duplicate file descriptor
pub fn sys_dup(_task: &Arc<Sel4Task>, _fd: usize) -> SysResult {
    Err(38) // ENOSYS
}

/// Duplicate file descriptor to specific number
pub fn sys_dup3(_task: &Arc<Sel4Task>, _oldfd: usize, _newfd: usize, _flags: u32) -> SysResult {
    Err(38) // ENOSYS
}

/// Readv - read into multiple buffers
pub fn sys_readv(_task: &Arc<Sel4Task>, _fd: usize, _iov_addr: usize, _iovcnt: usize) -> SysResult {
    Ok(0)
}

/// Writev - write from multiple buffers
pub fn sys_writev(task: &Arc<Sel4Task>, fd: usize, _iov_addr: usize, _iovcnt: usize) -> SysResult {
    // For stdout/stderr, just acknowledge
    if fd == 1 || fd == 2 {
        return Ok(0);
    }
    Ok(0)
}

/// pread64
pub fn sys_pread64(_task: &Arc<Sel4Task>, _fd: usize, _buf_addr: usize, _count: usize, _offset: isize) -> SysResult {
    Ok(0)
}

/// pwrite64
pub fn sys_pwrite64(_task: &Arc<Sel4Task>, _fd: usize, _buf_addr: usize, _count: usize, _offset: isize) -> SysResult {
    Ok(0)
}

/// sendfile
pub fn sys_sendfile(_task: &Arc<Sel4Task>, _out_fd: usize, _in_fd: usize, _offset_addr: usize, _count: usize) -> SysResult {
    Ok(0)
}

/// ppoll
pub fn sys_ppoll(_task: &Arc<Sel4Task>, _fds_addr: usize, _nfds: usize, _tmo_p: usize, _sigmask: usize) -> SysResult {
    Ok(0)
}

/// faccessat
pub fn sys_faccessat(task: &Arc<Sel4Task>, _dirfd: i32, path_addr: usize, mode: u32, _flags: u32) -> SysResult {
    let path = read_cstr_from_task(task, path_addr);
    match FS_CLIENT.access(&path, mode) {
        Ok(_) => Ok(0),
        Err(e) => Err(-e),
    }
}

/// utimensat
pub fn sys_utimensat(_task: &Arc<Sel4Task>, _dirfd: i32, _path_addr: usize, _times_addr: usize, _flags: u32) -> SysResult {
    Ok(0)
}

/// statfs
pub fn sys_statfs(_task: &Arc<Sel4Task>, _path_addr: usize, _buf_addr: usize) -> SysResult {
    Ok(0)
}

/// mount (stub)
pub fn sys_mount(_task: &Arc<Sel4Task>, _source: usize, _target: usize, _fstype: usize, _flags: usize, _data: usize) -> SysResult {
    Err(1) // EPERM
}

/// umount2 (stub)
pub fn sys_umount2(_task: &Arc<Sel4Task>, _target: usize, _flags: usize) -> SysResult {
    Err(1) // EPERM
}

/// Helper: read a null-terminated string from task memory
fn read_cstr_from_task(task: &Arc<Sel4Task>, addr: usize) -> alloc::string::String {
    let mut bytes = Vec::new();
    let mut a = addr;
    loop {
        let mut buf = [0u8; 1];
        if !task.read_bytes(a, &mut buf) || buf[0] == 0 {
            break;
        }
        bytes.push(buf[0]);
        a += 1;
        if bytes.len() > 4096 {
            break;
        }
    }
    alloc::string::String::from_utf8_lossy(&bytes).into_owned()
}
