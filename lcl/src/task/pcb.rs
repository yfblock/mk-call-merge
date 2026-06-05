//! Process control block - ported from kernel-thread

/// Process timer
#[derive(Default, Clone)]
pub struct ProcessTimer {
    pub interval_usec: u64,
    pub value_usec: u64,
}

/// Process control block
#[derive(Default)]
pub struct ProcessControlBlock {
    /// ITIMER_REAL timers
    pub itimer: [ProcessTimer; 3],
}

impl ProcessControlBlock {
    pub fn new() -> Self {
        Self {
            itimer: Default::default(),
        }
    }
}
