//! Root task IPC helpers

use sel4_sys::*;

/// Default parent endpoint slot
pub const DEFAULT_PARENT_EP: usize = 18;

/// Default serve endpoint slot
pub const DEFAULT_SERVE_EP: usize = 19;

/// Shutdown the system
pub fn shutdown() {
    seL4_DebugPutString("Shutting down...\n");
    seL4_DebugHalt();
}

/// Create a shared memory channel (stub - not supported in current project)
pub fn create_channel(_addr: usize, _page_count: usize) -> usize {
    0
}

/// Join a shared memory channel (stub)
pub fn join_channel(_channel_id: usize, _addr: usize) -> usize {
    0
}

/// Register an IRQ (stub)
pub fn register_irq(_irq: usize, _handler: usize) -> i32 {
    0
}

/// Register a notification (stub)
pub fn register_notify(_ntfn: usize, _badge: usize) -> core::result::Result<(), i32> {
    Ok(())
}

/// Translate address (stub)
pub fn translate_addr(_vaddr: usize) -> usize {
    0
}

/// Find a service by name (stub)
pub fn find_service(_name: &str) -> usize {
    0
}
