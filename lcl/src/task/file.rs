//! File descriptor management - ported from kernel-thread

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use spin::Mutex;

/// File descriptor entry
#[derive(Clone)]
pub struct FileDescriptor {
    pub path: String,
    pub flags: u32,
    pub offset: usize,
}

/// Task file info
#[derive(Default)]
pub struct TaskFileInfo {
    /// File descriptor table: fd -> FileDescriptor
    pub file_ds: Mutex<BTreeMap<usize, FileDescriptor>>,
    /// Working directory
    pub work_dir: Mutex<String>,
    /// Resource limits
    pub rlimit: Mutex<RLimit>,
}

/// Resource limits
#[derive(Default, Clone)]
pub struct RLimit {
    pub rlim_cur: u64,
    pub rlim_max: u64,
}

impl TaskFileInfo {
    pub fn new() -> Self {
        Self {
            file_ds: Mutex::new(BTreeMap::new()),
            work_dir: Mutex::new(String::from("/")),
            rlimit: Mutex::new(RLimit { rlim_cur: 1024, rlim_max: 1024 }),
        }
    }

    /// Open a file and return the file descriptor
    pub fn fd_open(&self, dirfd: i32, path: &str, flags: u32) -> Result<usize, i32> {
        let mut fds = self.file_ds.lock();
        let fd = fds.keys().max().map_or(3, |k| k + 1);
        fds.insert(fd, FileDescriptor {
            path: String::from(path),
            flags,
            offset: 0,
        });
        Ok(fd)
    }

    /// Resolve a file path relative to a directory fd
    pub fn fd_resolve(&self, dirfd: i32, path: &str) -> Result<String, i32> {
        if path.starts_with('/') {
            Ok(String::from(path))
        } else {
            let work_dir = self.work_dir.lock();
            let mut resolved = alloc::string::String::new();
            resolved.push_str(&work_dir);
            resolved.push('/');
            resolved.push_str(path);
            Ok(resolved)
        }
    }
}
