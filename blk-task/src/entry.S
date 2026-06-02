.section .text._start
.global _start
_start:
    /* RDI = IPC buffer 地址 (由 root-task 通过寄存器传递) */
    /* 设置栈 (8KB) */
    lea rsp, [stack_top]
    mov rbp, rsp

    /* 调用 Rust 入口 (RDI = IPC buffer 地址) */
    call rust_entry

    /* 不应该返回，如果返回则挂起 */
1:  hlt
    jmp 1b

.section .bss
.align 16
stack_bottom:
    .space 0x2000
stack_top:
