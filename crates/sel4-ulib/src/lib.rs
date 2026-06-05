//! sel4-ulib compatibility layer over sel4-sys
//!
//! Provides the high-level API (Cap types, MessageInfo, etc.) that kernel-thread expects,
//! backed by the pure-Rust sel4-sys crate.

#![no_std]
#![allow(unused)]
#![allow(dead_code)]

extern crate alloc;

// Re-export sel4-sys as sys
pub use sel4_sys as sys;

// Core types
pub use sel4_sys::{
    IpcBuffer, MessageInfo, MessageInfoBuilder, CapRights, ObjectType,
    with_ipc_buffer, set_ipc_buffer, ipc_buffer_addr,
    seL4_Send, seL4_NBSend, seL4_Call, seL4_Reply, seL4_Recv, seL4_NBRecv,
    seL4_ReplyRecv, seL4_Signal, seL4_Wait, seL4_Yield,
    seL4_DebugPutChar, seL4_DebugPutString, seL4_DebugHalt,
    seL4_TCB_Configure, seL4_TCB_WriteRegisters, seL4_TCB_SetSchedParams,
    seL4_TCB_BindNotification, seL4_TCB_UnbindNotification, seL4_TCB_SetTLSBase,
    seL4_CNode_Copy, seL4_CNode_Mint, seL4_CNode_Delete, seL4_CNode_Revoke,
    seL4_Untyped_Retype, seL4_Frame_Map, seL4_Frame_Unmap,
    seL4_PageTable_Map, seL4_PageTable_Unmap,
    seL4_PDPT_Map, seL4_PDPT_Unmap,
    seL4_ASIDPool_Assign, seL4_ASIDControl_MakePool,
    seL4_IRQControl_Get, seL4_IRQHandler_SetNotification, seL4_IRQHandler_Ack,
    seL4_X86_IOPortControl_Issue, seL4_X86_IOPort_Out8, seL4_X86_IOPort_Out16,
    seL4_X86_IOPort_Out32,
    init_slots,
};

/// CPtr type alias (raw capability pointer)
pub type CPtr = usize;

/// CPtrBits type alias
pub type CPtrBits = usize;

/// Badge type alias
pub type Badge = usize;

/// Capability wrapper providing typed methods
#[derive(Debug, Clone, Copy)]
pub struct Cap {
    pub cptr: CPtr,
}

impl Cap {
    pub const fn new(cptr: CPtr) -> Self {
        Self { cptr }
    }

    pub fn bits(self) -> CPtrBits {
        self.cptr
    }
}

/// Endpoint capability
pub mod cap {
    use super::*;
    use sel4_sys::*;

    pub type Endpoint = Cap;
    pub type Notification = Cap;
    pub type Tcb = Cap;
    pub type CNode = Cap;
    pub type Untyped = Cap;
    pub type IrqHandler = Cap;
    pub type IrqControl = Cap;
    pub type Frame = Cap;
    pub type PageTable = Cap;
    pub type VSpace = Cap;
}

/// Debug print macro
#[macro_export]
macro_rules! debug_print {
    ($($arg:tt)*) => {
        // No-op for now, could use seL4_DebugPutString
    };
}

/// Debug println macro
#[macro_export]
macro_rules! debug_println {
    () => {};
    ($($arg:tt)*) => {
        $crate::debug_print!($($arg)*);
        $crate::seL4_DebugPutChar(b'\n');
    };
}

/// with_ipc_buffer_mut alias
pub fn with_ipc_buffer_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut IpcBuffer) -> R,
{
    with_ipc_buffer(f)
}
