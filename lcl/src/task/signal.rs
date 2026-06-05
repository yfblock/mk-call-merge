//! Signal handling - ported from kernel-thread

use alloc::vec::Vec;

/// Signal action
#[derive(Default, Clone)]
pub struct SignalAction {
    pub handler: usize,
    pub flags: u32,
    pub mask: u64,
}

/// Task signal state
#[derive(Default)]
pub struct TaskSignal {
    /// Pending signals
    pub pending: u64,
    /// Signal mask
    pub mask: u64,
    /// Signal actions (65 entries for Linux)
    pub actions: Vec<SignalAction>,
    /// Exit signal
    pub exit_sig: Option<usize>,
}

impl TaskSignal {
    pub fn new() -> Self {
        let mut actions = Vec::new();
        actions.resize(65, SignalAction::default());
        Self {
            pending: 0,
            mask: 0,
            actions,
            exit_sig: None,
        }
    }

    /// Add a signal to pending
    pub fn add_signal(&mut self, sig: usize, _from_tid: usize) {
        if sig > 0 && sig <= 64 {
            self.pending |= 1 << (sig - 1);
        }
    }

    /// Check if there are pending unmasked signals
    pub fn has_unmasked_signal(&self) -> bool {
        (self.pending & !self.mask) != 0
    }

    /// Pop the next pending signal
    pub fn pop_signal(&mut self) -> Option<usize> {
        let pending = self.pending & !self.mask;
        if pending == 0 {
            return None;
        }
        let sig = pending.trailing_zeros() as usize + 1;
        self.pending &= !(1 << (sig - 1));
        Some(sig)
    }
}
