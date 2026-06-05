//! IPC helper macros

/// Read types from IPC buffer message registers
#[macro_export]
macro_rules! read_types {
    ($ib:expr, $t:ty) => {
        unsafe { core::ptr::read($ib.msg.as_ptr() as *const $t) }
    };
    ($ib:expr, $t1:ty, $t2:ty) => {{
        let ptr = $ib.msg.as_ptr();
        unsafe {
            (
                core::ptr::read(ptr as *const $t1),
                core::ptr::read(ptr.add(core::mem::size_of::<$t1>().div_ceil(core::mem::size_of::<usize>())) as *const $t2),
            )
        }
    }};
    ($t:ty) => {{
        use sel4_sys::with_ipc_buffer;
        with_ipc_buffer(|ib| unsafe { core::ptr::read(ib.msg.as_ptr() as *const $t) })
    }};
}

/// Reply with a value via IPC buffer
#[macro_export]
macro_rules! reply_with {
    ($ib:expr, $val:expr) => {{
        let val = $val;
        $ib.write_mr(0, val as usize);
        let reply = sel4_sys::MessageInfo::new(0, 1, 0);
        sel4_sys::seL4_Reply(reply.word());
    }};
}
