//! Memory management syscalls - ported from kernel-thread

use alloc::sync::Arc;
use common::config::PAGE_SIZE;
use crate::task::Sel4Task;
use crate::syscall::SysResult;

/// brk syscall - set program break
pub fn sys_brk(task: &Arc<Sel4Task>, new_brk: usize) -> SysResult {
    let mut mem = task.mem.lock();
    if new_brk == 0 {
        return Ok(mem.heap);
    }
    let old_heap = mem.heap;
    mem.heap = new_brk;
    drop(mem);

    // Map pages for heap growth
    if new_brk > old_heap {
        let start = old_heap & !(PAGE_SIZE - 1);
        let end = (new_brk + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        for vaddr in (start..end).step_by(PAGE_SIZE) {
            task.map_blank_page_simple(vaddr);
        }
    }

    Ok(new_brk)
}

/// mmap syscall - map memory
pub fn sys_mmap(task: &Arc<Sel4Task>, addr: usize, length: usize, _prot: usize) -> SysResult {
    let pages = (length + PAGE_SIZE - 1) / PAGE_SIZE;
    let start = if addr == 0 {
        task.find_free_area(0x100000, pages * PAGE_SIZE)
    } else {
        addr & !(PAGE_SIZE - 1)
    };

    // Map pages
    for i in 0..pages {
        task.map_blank_page_simple(start + i * PAGE_SIZE);
    }

    Ok(start)
}

/// munmap syscall - unmap memory
pub fn sys_munmap(task: &Arc<Sel4Task>, addr: usize, length: usize) -> SysResult {
    let start = addr & !(PAGE_SIZE - 1);
    let end = ((addr + length + PAGE_SIZE - 1) & !(PAGE_SIZE - 1));
    let mut mem = task.mem.lock();

    for vaddr in (start..end).step_by(PAGE_SIZE) {
        mem.mapped_page.remove(&vaddr);
        mem.page_data.remove(&vaddr);
    }

    Ok(0)
}
