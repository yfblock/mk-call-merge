.section .text._start
.global _start
_start:
    /* 设置栈 (16KB) */
    lea rsp, [stack_top]
    mov rbp, rsp

    /* 调用 Rust 入口 */
    call rust_entry

    /* 不应该返回 */
1:  hlt
    jmp 1b

.section .bss
.align 16
stack_bottom:
    .space 0x4000
stack_top:
