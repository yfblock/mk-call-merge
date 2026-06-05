//! Object allocator utilities

use alloc::vec::Vec;
use spin::Mutex;

/// Object allocator for managing seL4 capabilities
pub struct ObjectAllocator {
    pub recycled: Vec<usize>,
}

impl ObjectAllocator {
    pub const fn new() -> Self {
        Self { recycled: Vec::new() }
    }

    pub fn alloc(&mut self) -> Option<usize> {
        self.recycled.pop()
    }

    pub fn extend_slot(&mut self, slot: usize) {
        self.recycled.push(slot);
    }
}

/// Global object allocator
pub static OBJ_ALLOCATOR: Mutex<ObjectAllocator> = Mutex::new(ObjectAllocator::new());
