.section .text.boot, "ax"
.global _start
.type _start, %function

_start:
    // Initialize the stack pointer.
    ldr x0, =_stack_top
    mov sp, x0

    // Call the Rust kernel entry point.
    bl rust_main

1:
    wfe
    b 1b

.size _start, . - _start
