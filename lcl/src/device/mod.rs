//! Device module - ported from kernel-thread

pub mod uart;

/// Initialize devices
pub fn init() {
    // TODO: Initialize UART and other devices
    sel4_sys::seL4_DebugPutString("[lcl] Devices initialized\n");
}
