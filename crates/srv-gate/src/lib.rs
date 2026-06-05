//! Service gate - trait definitions for block, filesystem, and UART services

#![no_std]
#![allow(unused)]

extern crate alloc;

pub mod blk;
pub mod fs;
pub mod uart;
pub mod consts;

// Re-export trait definitions
pub use blk::{BlockIface, BlockIfaceEvent, BLK_IMPLS};
pub use fs::{FSIface, FSIfaceEvent};
pub use uart::{UartIface, UartIfaceEvent, UART_IMPLS};
