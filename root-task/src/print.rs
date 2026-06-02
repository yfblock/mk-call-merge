//! Serial output helpers using seL4 debug syscalls.

use sel4_sys::seL4_DebugPutChar;

/// Output a u64 as a decimal number to the kernel debug serial.
pub fn put_u64(val: u64) {
    if val == 0 {
        seL4_DebugPutChar(b'0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20;
    let mut v = val;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    for b in &buf[i..] {
        seL4_DebugPutChar(*b);
    }
}
