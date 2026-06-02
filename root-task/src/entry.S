// seL4 Root Task entry point and worker thread trampolines.
//
// _start: sets up stack + TLS, then calls the Rust entry.
// worker_ep_entry / worker_ntfn_entry: IPC benchmark worker loops.

.extern sel4_runtime_rust_entry

.global _start
.global worker_ep_entry
.global worker_ntfn_entry

// Stack and TLS area in .bss section (mapped by kernel as part of ELF)
.section .bss
.align 16
stack_bottom:
    .space 0x10000
stack_top:
.align 16
tls_area:
    .space 0x1000

.section .text

_start:
    // Set up FS register for TLS using WRFSBASE
    lea rax, [tls_area]
    wrfsbase rax
    // Set up stack within .bss section (mapped by kernel)
    lea rsp, [stack_top]
    mov rbp, rsp
    sub rsp, 0x8          // Stack must be 16-byte aligned before call
    push rbp
    // rdi = bi_frame_vptr (BootInfo frame address, set by kernel)
    call sel4_runtime_rust_entry

1:  jmp 1b

// ---------------------------------------------------------------------------
// Worker thread entry points for IPC benchmarking.
//
// These run on a separate stack (set up via TCB_WriteRegisters).
// R12 is preserved across syscalls and holds the capability slot.
// RDI is clobbered by the kernel (badge on Recv), so we reload it from R12.
// ---------------------------------------------------------------------------

// Endpoint worker: ReplyRecv loop
// R12 = endpoint cap (set by WriteRegisters, preserved across syscall)
worker_ep_entry:
    mov rdi, r12              // restore endpoint cap (clobbered by recv badge)
    mov rsi, 0                // msginfo = 0 (no message)
    mov rdx, -2               // SYS_REPLY_RECV
    syscall
    jmp worker_ep_entry

// Notification worker: Wait (Recv) loop
// R12 = notification cap (set by WriteRegisters, preserved across syscall)
worker_ntfn_entry:
    mov rdi, r12              // restore notification cap
    mov rsi, 0                // msginfo = 0
    mov rdx, -5               // SYS_RECV
    syscall
    jmp worker_ntfn_entry
