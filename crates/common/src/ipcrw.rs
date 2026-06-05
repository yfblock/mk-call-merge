//! IPC read/write primitives

use sel4_sys::IpcBuffer;

/// Read a value from IPC buffer at given byte offset
pub unsafe fn read_from_ipc<T: Copy>(ib: &IpcBuffer, byte_offset: usize) -> T {
    unsafe {
        core::ptr::read((ib.msg.as_ptr() as *const u8).add(byte_offset) as *const T)
    }
}

/// Write a value to IPC buffer at given byte offset
pub unsafe fn write_to_ipc<T: Copy>(ib: &mut IpcBuffer, byte_offset: usize, val: T) {
    unsafe {
        core::ptr::write((ib.msg.as_mut_ptr() as *mut u8).add(byte_offset) as *mut T, val);
    }
}
