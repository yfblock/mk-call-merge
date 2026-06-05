/* ext4-srv entry point - x86_64 seL4 task */

.section .text._start
.global _start
.type _start, @function

_start:
    /* seL4 passes boot info pointer in RDI */
    /* Set up a small stack */
    lea rsp, [rip + stack_top]

    /* Call Rust entry point */
    call sel4_runtime_rust_entry

    /* Should never return */
    ud2

.section .bss
.align 16
stack_bottom:
    .space 0x4000  /* 16KB stack */
stack_top:
