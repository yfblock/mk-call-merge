//! Child task management

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;
use crate::task::Sel4Task;

/// ArcTask type alias
pub type ArcTask = Arc<Sel4Task>;

/// Global task map: tid -> ArcTask
pub static TASK_MAP: Mutex<BTreeMap<usize, ArcTask>> = Mutex::new(BTreeMap::new());

/// Futex table entry
pub type FutexEntry = (usize, usize);
/// Futex table type
pub type FutexTable = Vec<FutexEntry>;

/// Wake hangups for a task
pub fn wake_hangs(_task: &Sel4Task) {
    // TODO: implement
}

/// Wake futex waiters
pub fn futex_wake(_table: Arc<Mutex<FutexTable>>, _uaddr: usize, _count: usize) {
    // TODO: implement
}
