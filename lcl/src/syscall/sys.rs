//! System info syscalls - ported from kernel-thread

use alloc::sync::Arc;
use crate::task::Sel4Task;
use crate::syscall::SysResult;

/// Linux utsname structure (x86_64)
#[repr(C)]
struct UtsName {
    sysname: [u8; 65],
    nodename: [u8; 65],
    release: [u8; 65],
    version: [u8; 65],
    machine: [u8; 65],
    domainname: [u8; 65],
}

impl Default for UtsName {
    fn default() -> Self {
        Self {
            sysname: [0u8; 65], nodename: [0u8; 65], release: [0u8; 65],
            version: [0u8; 65], machine: [0u8; 65], domainname: [0u8; 65],
        }
    }
}

/// uname syscall - return system information
pub fn sys_uname(task: &Arc<Sel4Task>, buf_addr: usize) -> SysResult {
    let mut utsname = UtsName::default();

    let sysname = b"Linux";
    let nodename = b"lcl";
    let release = b"6.1.0-lcl";
    let version = b"#1 SMP x86_64";
    let machine = b"x86_64";

    utsname.sysname[..sysname.len()].copy_from_slice(sysname);
    utsname.nodename[..nodename.len()].copy_from_slice(nodename);
    utsname.release[..release.len()].copy_from_slice(release);
    utsname.version[..version.len()].copy_from_slice(version);
    utsname.machine[..machine.len()].copy_from_slice(machine);

    // Write to task memory
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &utsname as *const UtsName as *const u8,
            core::mem::size_of::<UtsName>(),
        )
    };
    task.write_bytes(buf_addr, bytes);
    Ok(0)
}

/// gettimeofday syscall
pub fn sys_gettimeofday(task: &Arc<Sel4Task>, tv_addr: usize) -> SysResult {
    // Write zeros for now
    task.write_bytes(tv_addr, &0u64.to_le_bytes());
    task.write_bytes(tv_addr + 8, &0u64.to_le_bytes());
    Ok(0)
}

/// clock_gettime syscall
pub fn sys_clock_gettime(task: &Arc<Sel4Task>, _clock_id: usize, ts_addr: usize) -> SysResult {
    // Write zeros for now
    task.write_bytes(ts_addr, &0u64.to_le_bytes());
    task.write_bytes(ts_addr + 8, &0u64.to_le_bytes());
    Ok(0)
}
