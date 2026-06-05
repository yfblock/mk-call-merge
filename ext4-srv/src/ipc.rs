//! IPC protocol for ext4 filesystem service

/// Request messages from clients
#[repr(C)]
#[derive(Debug, Clone)]
pub enum FsRequest {
    /// Open a file: (path, flags) -> fd
    Open { path_addr: usize, path_len: usize, flags: u32 },
    /// Read from file: (fd, buf_addr, count) -> bytes_read
    Read { fd: usize, buf_addr: usize, count: usize },
    /// Write to file: (fd, data_addr, count) -> bytes_written
    Write { fd: usize, data_addr: usize, count: usize },
    /// Close file: (fd) -> ok
    Close { fd: usize },
    /// Get file status: (path_addr, path_len, stat_addr) -> ok
    Stat { path_addr: usize, path_len: usize, stat_addr: usize },
    /// Read directory entries: (fd, buf_addr, count) -> (bytes_read, new_offset)
    Getdents64 { fd: usize, buf_addr: usize, count: usize },
    /// Create directory: (path_addr, path_len) -> ok
    Mkdir { path_addr: usize, path_len: usize },
    /// Remove file: (path_addr, path_len) -> ok
    Unlink { path_addr: usize, path_len: usize },
    /// Check if file exists: (path_addr, path_len) -> bool
    Access { path_addr: usize, path_len: usize, mode: u32 },
    /// Get file size: (fd) -> size
    FileSize { fd: usize },
}

/// Response messages to clients
#[repr(C)]
#[derive(Debug, Clone)]
pub enum FsResponse {
    /// Success with result value
    Ok(usize),
    /// Success with two result values (for getdents64)
    Ok2(usize, usize),
    /// Error with errno
    Err(i32),
}

/// IPC message wrapper
#[repr(C)]
#[derive(Debug, Clone)]
pub struct FsMessage {
    pub request: FsRequest,
}

/// seL4 endpoint capability slot for the filesystem service
pub const FS_ENDPOINT_SLOT: usize = 100;

/// Maximum path length
pub const MAX_PATH_LEN: usize = 256;
