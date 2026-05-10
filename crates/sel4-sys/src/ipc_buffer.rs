//! IPC buffer representation and operations.
//!
//! Each seL4 thread has a dedicated IPC buffer page. This module defines the
//! layout of the buffer in memory and provides safe accessors.

use crate::types::{IPC_BUFFER_MSG_REGS, UserContext};

/// Represents the seL4 IPC buffer mapped into the thread's address space.
///
/// The IPC buffer is used for:
/// - Transferring message registers beyond those that fit in CPU registers
/// - Transferring capability references (badges)
/// - Receiving capabilities
/// - Storing user-defined data
///
/// # Layout
///
/// The buffer is laid out in memory as:
/// - `msg[0..120]`: Extended message registers (word-sized)
/// - `user_data`: Free-form user data word
/// - `caps_or_badges[0..3]`: Capability transfer info
/// - `receive_cnode`, `receive_index`, `receive_depth`: Capability receive slot
///
/// Total size: 4096 bytes (one page).
#[repr(C, align(4096))]
pub struct IpcBuffer {
    /// Message registers beyond those in CPU registers.
    ///
    /// On x86_64, CPU registers carry MR0-MR5 (rdi, rsi, r10, r8, r9, r12).
    /// Additional MRs (6..) are read from this array when the message info
    /// `length` exceeds the number of physical registers.
    pub msg: [usize; IPC_BUFFER_MSG_REGS],

    /// User-defined data word (free for application use).
    pub user_data: usize,

    /// Capability transfer / badge info words.
    pub caps_or_badges: [usize; 3],

    /// CNode to receive capabilities into.
    pub receive_cnode: usize,

    /// Index in the receive CNode.
    pub receive_index: usize,

    /// Depth of the receive slot.
    pub receive_depth: usize,

    /// Padding to fill the rest of the page.
    _padding: [u8; IPC_BUFFER_PADDING],
}

/// Calculate padding bytes to fill the IPC buffer to exactly one page.
const IPC_BUFFER_PADDING: usize = {
    let header_size = IPC_BUFFER_MSG_REGS * 8  // msg array
        + 8                                     // user_data
        + 3 * 8                                 // caps_or_badges
        + 3 * 8;                                // receive fields
    crate::types::IPC_BUFFER_SIZE - header_size
};

impl IpcBuffer {
    /// Create a zero-initialized IPC buffer.
    pub const fn new() -> Self {
        Self {
            msg: [0; IPC_BUFFER_MSG_REGS],
            user_data: 0,
            caps_or_badges: [0; 3],
            receive_cnode: 0,
            receive_index: 0,
            receive_depth: 0,
            _padding: [0; IPC_BUFFER_PADDING],
        }
    }

    /// Set up the receive slot for capability transfer.
    ///
    /// When receiving a message that includes capabilities, the kernel will
    /// place the received capabilities into this slot.
    pub fn set_receive_slot(&mut self, cnode: usize, index: usize, depth: usize) {
        self.receive_cnode = cnode;
        self.receive_index = index;
        self.receive_depth = depth;
    }

    /// Read a message register from the IPC buffer.
    ///
    /// Note: MR0-MR5 are typically in CPU registers, not here.
    pub fn read_mr(&self, idx: usize) -> usize {
        if idx < IPC_BUFFER_MSG_REGS {
            self.msg[idx]
        } else {
            0
        }
    }

    /// Write a message register to the IPC buffer.
    pub fn write_mr(&mut self, idx: usize, val: usize) {
        if idx < IPC_BUFFER_MSG_REGS {
            self.msg[idx] = val;
        }
    }

    /// Write a UserContext to the IPC buffer (for TCB_WriteRegisters).
    ///
    /// The user context (20 registers on x86_64) is written starting at the
    /// beginning of the msg area.
    pub fn write_user_context(&mut self, ctx: &UserContext) {
        let bytes = ctx.as_bytes();
        let words = unsafe {
            core::slice::from_raw_parts(bytes.as_ptr() as *const usize, bytes.len() / 8)
        };
        for (i, &w) in words.iter().enumerate() {
            if i < IPC_BUFFER_MSG_REGS {
                self.msg[i] = w;
            }
        }
    }

    /// Read a UserContext from the IPC buffer.
    pub fn read_user_context(&self) -> UserContext {
        let mut ctx = UserContext::default();
        let bytes = ctx.as_bytes_mut();
        let words = unsafe {
            core::slice::from_raw_parts_mut(bytes.as_mut_ptr() as *mut usize, bytes.len() / 8)
        };
        for (i, w) in words.iter_mut().enumerate() {
            if i < IPC_BUFFER_MSG_REGS {
                *w = self.msg[i];
            }
        }
        ctx
    }

    /// Write a slice of words into the message registers.
    pub fn write_words(&mut self, words: &[usize]) {
        for (i, &w) in words.iter().enumerate() {
            if i < IPC_BUFFER_MSG_REGS {
                self.msg[i] = w;
            }
        }
    }

    /// Read a slice of words from the message registers.
    pub fn read_words(&self, count: usize) -> &[usize] {
        let count = count.min(IPC_BUFFER_MSG_REGS);
        &self.msg[..count]
    }
}

/// Assert that the IPC buffer size matches the expected page size.
const _: () = {
    if core::mem::size_of::<IpcBuffer>() != crate::types::IPC_BUFFER_SIZE {
        panic!("IpcBuffer size does not match IPC_BUFFER_SIZE");
    }
};
