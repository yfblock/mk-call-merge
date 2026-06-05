//! Thread/process syscalls - ported from kernel-thread

use alloc::sync::Arc;
use crate::task::Sel4Task;
use crate::syscall::SysResult;

/// Get process ID
pub fn sys_getpid(task: &Arc<Sel4Task>) -> SysResult {
    Ok(task.pid)
}

/// Get thread ID
pub fn sys_gettid(task: &Arc<Sel4Task>) -> SysResult {
    Ok(task.tid)
}

/// Get parent process ID
pub fn sys_getppid(task: &Arc<Sel4Task>) -> SysResult {
    Ok(task.ppid)
}

/// Set thread ID address
pub fn sys_set_tid_address(task: &Arc<Sel4Task>, addr: usize) -> SysResult {
    *task.clear_child_tid.lock() = addr;
    Ok(task.tid)
}

/// Exit process
pub fn sys_exit(task: &Arc<Sel4Task>, exit_code: u32) -> SysResult {
    task.exit_with(exit_code << 8);
    Ok(0)
}

/// Exit group
pub fn sys_exit_group(task: &Arc<Sel4Task>, exit_code: u32) -> SysResult {
    task.exit_with(exit_code << 8);
    Ok(0)
}

/// Clone (create new thread/process)
pub fn sys_clone(task: &Arc<Sel4Task>, flags: usize, newsp: usize, _parent_tid: usize, _child_tid: usize, _tls: usize) -> SysResult {
    // For now, return error - clone is complex
    Err(38) // ENOSYS
}

/// Execve (replace current process)
pub fn sys_execve(task: &Arc<Sel4Task>, _filename: usize, _argv: usize, _envp: usize) -> SysResult {
    // For now, return error
    Err(38) // ENOSYS
}

/// Wait for process
pub fn sys_wait4(task: &Arc<Sel4Task>, _pid: isize, _status: usize, _options: u32) -> SysResult {
    // For now, return error
    Err(10) // ECHILD
}

/// Futex
pub fn sys_futex(task: &Arc<Sel4Task>, _uaddr: usize, _op: i32, _val: u32, _timeout: usize, _uaddr2: usize, _val3: u32) -> SysResult {
    // For now, return 0
    Ok(0)
}

/// Get resource usage
pub fn sys_getrusage(_task: &Arc<Sel4Task>, _who: usize, _usage: usize) -> SysResult {
    Ok(0)
}

/// Get resource limits
pub fn sys_prlimit64(_task: &Arc<Sel4Task>, _pid: usize, _resource: usize, _new_limit: usize, _old_limit: usize) -> SysResult {
    Ok(0)
}
