//! Capability slot allocator and simple mutex for the root task.

/// A simple capability slot allocator.
///
/// Allocates slots sequentially from a free range. Recycling (freeing) is
/// not yet supported — slots are consumed monotonically.
pub struct SlotManager {
    next: usize,
    end: usize,
}

impl SlotManager {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { next: start, end }
    }

    pub fn alloc(&mut self) -> Option<usize> {
        if self.next >= self.end {
            return None;
        }
        let slot = self.next;
        self.next += 1;
        Some(slot)
    }

    pub fn available(&self) -> usize {
        self.end - self.next
    }
}

/// Global slot manager. Protected by a simple spin mutex.
pub static SLOT_MANAGER: SimpleMutex<SlotManager> =
    SimpleMutex::new(SlotManager::new(256, 0x1000));

// ---------------------------------------------------------------------------
// Simple mutex (busy-wait, for single-threaded use)
// ---------------------------------------------------------------------------

pub struct SimpleMutex<T> {
    locked: core::cell::Cell<bool>,
    data: core::cell::UnsafeCell<T>,
}

// SAFETY: Single-threaded root task.
unsafe impl<T> Sync for SimpleMutex<T> {}

impl<T> SimpleMutex<T> {
    pub const fn new(data: T) -> Self {
        Self {
            locked: core::cell::Cell::new(false),
            data: core::cell::UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> SimpleMutexGuard<'_, T> {
        while self.locked.replace(true) {
            core::hint::spin_loop();
        }
        SimpleMutexGuard { mutex: self }
    }
}

pub struct SimpleMutexGuard<'a, T> {
    mutex: &'a SimpleMutex<T>,
}

impl<T> core::ops::Deref for SimpleMutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<T> core::ops::DerefMut for SimpleMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<T> Drop for SimpleMutexGuard<'_, T> {
    fn drop(&mut self) {
        self.mutex.locked.set(false);
    }
}
