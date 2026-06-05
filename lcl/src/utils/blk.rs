//! Block device utilities - ported from kernel-thread

use blk_task::{BlockIface, BLOCK_SIZE, BLK};

/// Read blocks from the block device
pub fn read_blocks(block_id: usize, block_num: usize, buf: &mut [u8]) {
    for i in 0..block_num {
        let offset = i * BLOCK_SIZE;
        BLK.read_block(block_id + i, &mut buf[offset..offset + BLOCK_SIZE]);
    }
}

/// Write blocks to the block device
pub fn write_blocks(block_id: usize, block_num: usize, buf: &[u8]) {
    for i in 0..block_num {
        let offset = i * BLOCK_SIZE;
        BLK.write_block(block_id + i, &buf[offset..offset + BLOCK_SIZE]);
    }
}

/// Get block device capacity in bytes
pub fn capacity() -> u64 {
    BLK.capacity()
}
