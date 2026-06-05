//! UART interface

use alloc::sync::Arc;
use spin::{Lazy, Mutex};

/// UART interface trait
pub trait UartIface: Sync + Send {
    fn init(&mut self);
    fn putchar(&self, c: u8);
    fn getchar(&self) -> u8;
    fn puts(&self, data: &[u8]);
}

/// UART events
#[repr(usize)]
#[derive(Debug, Clone, Copy)]
pub enum UartIfaceEvent {
    init = 0,
    putchar = 1,
    getchar = 2,
    puts = 3,
}

impl TryFrom<usize> for UartIfaceEvent {
    type Error = ();
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::init),
            1 => Ok(Self::putchar),
            2 => Ok(Self::getchar),
            3 => Ok(Self::puts),
            _ => Err(()),
        }
    }
}

/// Global UART implementations
pub static UART_IMPLS: spin::Mutex<alloc::vec::Vec<Arc<Mutex<dyn UartIface>>>> =
    spin::Mutex::new(alloc::vec::Vec::new());

/// Register a UART implementation
#[macro_export]
macro_rules! def_uart_impl {
    ($name:ident, $expr:expr) => {
        // Simplified version
    };
}
