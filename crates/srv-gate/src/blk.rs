//! Block device interface

use alloc::sync::Arc;
use spin::{Lazy, Mutex};

/// Block device interface trait
pub trait BlockIface: Sync + Send {
    fn init(&mut self, channel_id: usize);
    fn read_block(&mut self, block_id: usize, block_num: usize);
    fn write_block(&mut self, block_id: usize, block_num: usize);
    fn capacity(&self) -> u64;
}

/// Block device events
#[repr(usize)]
#[derive(Debug, Clone, Copy)]
pub enum BlockIfaceEvent {
    init = 0,
    read_block = 1,
    write_block = 2,
    capacity = 3,
}

impl TryFrom<usize> for BlockIfaceEvent {
    type Error = ();
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::init),
            1 => Ok(Self::read_block),
            2 => Ok(Self::write_block),
            3 => Ok(Self::capacity),
            _ => Err(()),
        }
    }
}

/// Global block device implementations
pub static BLK_IMPLS: spin::Mutex<alloc::vec::Vec<Arc<Mutex<dyn BlockIface>>>> =
    spin::Mutex::new(alloc::vec::Vec::new());

/// Register a block device implementation
#[macro_export]
macro_rules! def_blk_impl {
    ($name:ident, $expr:expr) => {
        // This is a simplified version - in the real srv-gate it uses linkme
    };
}
