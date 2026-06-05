//! Timer handling - ported from kernel-thread
//!
//! Provides hardware timer support and sleep/wait functionality.

use alloc::vec::Vec;
use core::time::Duration;
use spin::Mutex;

use crate::child_test::TASK_MAP;
use crate::task::PollWakeEvent;

/// Timer type
enum TimerType {
    /// Wait for time (tid, target_duration_ms)
    WaitTime(usize, u64),
    /// Process timer (pid)
    ITimer(usize),
}

/// Time queue: (target_time_ms, timer_type)
static TIME_QUEUE: Mutex<Vec<(u64, TimerType)>> = Mutex::new(Vec::new());

/// Current system time in milliseconds (simplified)
static CURRENT_TIME_MS: Mutex<u64> = Mutex::new(0);

/// Get current time in milliseconds
pub fn current_time_ms() -> u64 {
    *CURRENT_TIME_MS.lock()
}

/// Advance time (called from timer interrupt handler)
pub fn advance_time(ms: u64) {
    *CURRENT_TIME_MS.lock() += ms;
}

/// Initialize timer
pub fn init() {
    // TODO: Register timer IRQ with seL4
    // For now, timer is software-only
    sel4_sys::seL4_DebugPutString("[lcl] Timer initialized\n");
}

/// Handle timer interrupt
pub fn handle_timer() {
    let curr_time = current_time_ms();

    // Process expired timers
    TIME_QUEUE.lock().retain(|(target_time, timer_ty)| {
        if curr_time >= *target_time {
            match timer_ty {
                TimerType::WaitTime(tid, _) => {
                    // Wake the waiting task
                    let map = TASK_MAP.lock();
                    if let Some(task) = map.get(tid) {
                        // Mark task as ready
                    }
                }
                TimerType::ITimer(pid) => {
                    // Send SIGALRM to process
                    let map = TASK_MAP.lock();
                    if let Some(task) = map.get(pid) {
                        let mut signal = task.signal.lock();
                        signal.pending |= 1 << 13; // SIGALRM = 14, bit 13
                    }
                }
            };
            false // Remove from queue
        } else {
            true // Keep in queue
        }
    });
}

/// Sleep for a duration (synchronous version)
pub fn sleep_ms(ms: u64) {
    let target = current_time_ms() + ms;
    while current_time_ms() < target {
        // Busy wait (in real implementation, would yield to scheduler)
        core::hint::spin_loop();
    }
}

/// Set a process timer (ITIMER_REAL)
pub fn set_process_timer(pid: usize, target_ms: u64) {
    TIME_QUEUE.lock().push((target_ms, TimerType::ITimer(pid)));
    TIME_QUEUE.lock().sort_by(|a, b| a.0.cmp(&b.0));
}
