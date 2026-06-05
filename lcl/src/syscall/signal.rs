//! Signal syscalls - ported from kernel-thread

use alloc::sync::Arc;
use alloc::vec;
use crate::task::Sel4Task;
use crate::child_test::TASK_MAP;
use crate::syscall::SysResult;

/// sigprocmask - change blocked signals
pub fn sys_sigprocmask(task: &Arc<Sel4Task>, how: i32, set_addr: usize, old_addr: usize) -> SysResult {
    let mut signal = task.signal.lock();

    if old_addr != 0 {
        task.write_bytes(old_addr, &signal.mask.to_le_bytes());
    }

    if set_addr != 0 {
        let mut buf = [0u8; 8];
        task.read_bytes(set_addr, &mut buf);
        let new_mask = u64::from_le_bytes(buf);

        match how {
            0 => signal.mask |= new_mask,      // SIG_BLOCK
            1 => signal.mask &= !new_mask,     // SIG_UNBLOCK
            2 => signal.mask = new_mask,        // SIG_SETMASK
            _ => return Err(22),
        }
    }

    Ok(0)
}

/// rt_sigaction - change signal action
pub fn sys_rt_sigaction(task: &Arc<Sel4Task>, sig: usize, act_addr: usize, oldact_addr: usize, _sigsetsize: usize) -> SysResult {
    if sig == 0 || sig > 64 {
        return Err(22);
    }

    let mut signal = task.signal.lock();

    if oldact_addr != 0 {
        let action = &signal.actions[sig - 1];
        let bytes = unsafe {
            core::slice::from_raw_parts(action as *const _ as *const u8, 32)
        };
        task.write_bytes(oldact_addr, bytes);
    }

    if act_addr != 0 {
        let mut buf = vec![0u8; 32];
        task.read_bytes(act_addr, &mut buf);
        // Parse handler, flags, mask from buf
        let handler = usize::from_le_bytes(buf[0..8].try_into().unwrap_or([0; 8]));
        signal.actions[sig - 1].handler = handler;
    }

    Ok(0)
}

/// rt_sigprocmask - real-time version
pub fn sys_rt_sigprocmask(task: &Arc<Sel4Task>, how: i32, set_addr: usize, old_addr: usize, _sigsetsize: usize) -> SysResult {
    sys_sigprocmask(task, how, set_addr, old_addr)
}

/// rt_sigreturn - return from signal handler
pub fn sys_rt_sigreturn(_task: &Arc<Sel4Task>, _regs: &mut [usize; 20]) -> SysResult {
    Ok(0)
}

/// kill - send signal to process
pub fn sys_kill(_task: &Arc<Sel4Task>, pid: usize, sig: usize) -> SysResult {
    if sig == 0 {
        let map = TASK_MAP.lock();
        return if map.contains_key(&pid) { Ok(0) } else { Err(3) };
    }

    let map = TASK_MAP.lock();
    if let Some(target) = map.get(&pid) {
        let mut signal = target.signal.lock();
        if sig <= 64 {
            signal.pending |= 1 << (sig - 1);
        }
        Ok(0)
    } else {
        Err(3)
    }
}

/// tkill - send signal to thread
pub fn sys_tkill(_task: &Arc<Sel4Task>, tid: usize, sig: usize) -> SysResult {
    let map = TASK_MAP.lock();
    if let Some(target) = map.get(&tid) {
        let mut signal = target.signal.lock();
        if sig <= 64 {
            signal.pending |= 1 << (sig - 1);
        }
        Ok(0)
    } else {
        Err(3)
    }
}
