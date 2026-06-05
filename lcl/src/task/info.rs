//! Task info - ported from kernel-thread

use alloc::string::String;
use alloc::vec::Vec;

/// Task initial info
#[derive(Default, Clone)]
pub struct TaskInfo {
    /// Entry point address
    pub entry: usize,
    /// End of task virtual memory
    pub task_vm_end: usize,
    /// Arguments
    pub args: Vec<String>,
    /// Environment variables
    pub envs: Vec<String>,
}
