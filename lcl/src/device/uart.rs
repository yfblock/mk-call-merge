//! UART device - ported from kernel-thread

use sel4_sys::*;

/// COM1 port address
const COM1: u16 = 0x3f8;

/// Initialize UART
pub fn init() {
    // UART is already initialized by seL4 kernel
    // Just verify it's working
    seL4_DebugPutString("[lcl] UART initialized\n");
}

/// Read a byte from port
#[inline]
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    core::arch::asm!(
        "in al, dx",
        out("al") val,
        in("dx") port,
        options(nomem, nostack, preserves_flags)
    );
    val
}

/// Write a byte to port
#[inline]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!(
        "out dx, al",
        in("al") val,
        in("dx") port,
        options(nomem, nostack, preserves_flags)
    );
}

/// Read a character from UART (blocking)
pub fn get_char() -> Option<u8> {
    let lsr = unsafe { inb(COM1 + 5) };
    if lsr & 0x01 != 0 {
        Some(unsafe { inb(COM1) })
    } else {
        None
    }
}

/// Write a character to UART
pub fn put_char(c: u8) {
    loop {
        let lsr = unsafe { inb(COM1 + 5) };
        if lsr & 0x20 != 0 {
            break;
        }
    }
    unsafe { outb(COM1, c) };
}

/// Write a string to UART
pub fn puts(s: &str) {
    for byte in s.bytes() {
        put_char(byte);
    }
}
