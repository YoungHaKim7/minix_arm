// aarch64 boot and exception entry for the MINIX Rust port.
//
// This is the counterpart of minix/minix/kernel/arch/*/: early boot, the
// exception vector table (kernel/arch/*/exception.S / mpx.S) and the
// context switch path (switch_to_user / restart).
//
// QEMU virt machine, single CPU (no SMP port yet), kernel at 0x40080000.

.section .text.boot, "ax"
.global _start
.type _start, %function
_start:
    // Debug: mark boot progress on the UART (PL011 on QEMU virt).
    ldr x9, =0x09000000
    mov w10, #0x42               // 'B'
    strb w10, [x9]
    // QEMU -kernel enters at EL2 on the virt machine (no EL3 firmware).
    mrs x0, CurrentEL
    lsr x0, x0, #2
    cmp x0, #2
    b.eq .Ldrop_to_el1
    cmp x0, #1
    b.eq .Lel1
    // EL3 not expected on QEMU virt; hang otherwise.
1:  wfe
    b 1b

.Ldrop_to_el1:
    // Clean EL1 environment: no stage-2 translation, no trap routing,
    // no virtual interrupt injection (HCR_EL2 = 0).
    msr hcr_el2, xzr
    // Let EL1 access the physical counter and timer (CNTHCTL_EL2).
    mov x0, #3
    msr cnthctl_el2, x0
    // EL1h after eret uses SP_EL1: give it the boot stack.
    ldr x1, =_stack_top
    msr sp_el1, x1
    mov x0, #0x3c5              // DAIF masked, mode EL1h
    msr spsr_el2, x0
    adr x0, .Lel1
    msr elr_el2, x0
    eret

.Lel1:
    // Debug: reached EL1.
    ldr x9, =0x09000000
    mov w10, #0x31               // '1'
    strb w10, [x9]
    msr spsel, #1               // use SP_EL1 from here on
    ldr x0, =_stack_top
    mov sp, x0
    bl rust_main
1:  wfe
    b 1b
.size _start, . - _start

// IDLE process body: ports idle() in kernel/proc.c - run when no one
// else can run.
.section .text
.global idle_entry
idle_entry:
1:  wfe
    b 1b

// ---------------------------------------------------------------------------
// Exception vector table (ports kernel/arch/*/exception.S).
// VBAR_EL1 requires 2KB alignment; 16 slots of 128 bytes.
// Frame layout (struct StackFrame in src/arch.rs), 272 bytes:
//   [  0..240)  x0 .. x30
//   [240..248)  padding
//   [248]       sp_el0
//   [256]       elr_el1 (pc)
//   [264]       spsr_el1 (pstate)
// ---------------------------------------------------------------------------
.section .text.vectors, "ax"
.balign 2048
.global vector_table
vector_table:
// Each vector slot is 128 bytes: branch, then pad to the next slot.
.macro VEC target
    b \target
    .space 0x80 - 4
.endm
    // 0x000: current EL, using SP0 - not used (kernel runs with SPSel=1)
    VEC .Ltrap_unexpected      // sync
    VEC .Ltrap_unexpected      // irq
    VEC .Ltrap_unexpected      // fiq
    VEC .Ltrap_unexpected      // SError
    // 0x200: current EL, using SPx - exceptions from kernel context (IDLE)
    VEC .Lexc_current_sync     // sync
    VEC .Lexc_irq              // irq
    VEC .Ltrap_unexpected      // fiq
    VEC .Ltrap_unexpected      // SError
    // 0x400: lower EL, AArch64 - exceptions from user processes (EL0)
    VEC .Lexc_lower_sync       // sync
    VEC .Lexc_irq              // irq
    VEC .Ltrap_unexpected      // fiq
    VEC .Ltrap_unexpected      // SError
    // 0x600: lower EL, AArch32 - unused
    VEC .Ltrap_unexpected
    VEC .Ltrap_unexpected
    VEC .Ltrap_unexpected
    VEC .Ltrap_unexpected

// kind codes passed to trap_dispatch
    .equ TRAP_LOWER_SYNC, 0     // SVC (or fault) from EL0
    .equ TRAP_CURRENT_SYNC, 1   // sync exception at EL1 (kernel bug)
    .equ TRAP_IRQ, 2            // interrupt, from EL0 or EL1

.macro SAVE_ALL
    sub sp, sp, #272
    stp x0, x1,   [sp, #0]
    stp x2, x3,   [sp, #16]
    stp x4, x5,   [sp, #32]
    stp x6, x7,   [sp, #48]
    stp x8, x9,   [sp, #64]
    stp x10, x11, [sp, #80]
    stp x12, x13, [sp, #96]
    stp x14, x15, [sp, #112]
    stp x16, x17, [sp, #128]
    stp x18, x19, [sp, #144]
    stp x20, x21, [sp, #160]
    stp x22, x23, [sp, #176]
    stp x24, x25, [sp, #192]
    stp x26, x27, [sp, #208]
    stp x28, x29, [sp, #224]
    str x30,      [sp, #240]
    mrs x9, sp_el0
    str x9,       [sp, #248]
    mrs x9, elr_el1
    mrs x10, spsr_el1
    stp x9, x10,  [sp, #256]
.endm

// Restore the context pointed to by \fr and eret into it.  The context
// always sits at the top of the target process's kernel stack, so
// SP_EL1 is set to fr+272; this ports restart()/switch_to_user().
.macro RESUME_ALL fr
    mov x30, \fr
    add x9, x30, #272
    mov sp, x9
    ldr x10, [x30, #248]
    msr sp_el0, x10
    ldp x10, x11, [x30, #256]
    msr elr_el1, x10
    msr spsr_el1, x11
    ldp x0, x1,   [x30, #0]
    ldp x2, x3,   [x30, #16]
    ldp x4, x5,   [x30, #32]
    ldp x6, x7,   [x30, #48]
    ldp x8, x9,   [x30, #64]
    ldp x10, x11, [x30, #80]
    ldp x12, x13, [x30, #96]
    ldp x14, x15, [x30, #112]
    ldp x16, x17, [x30, #128]
    ldp x18, x19, [x30, #144]
    ldp x20, x21, [x30, #160]
    ldp x22, x23, [x30, #176]
    ldp x24, x25, [x30, #192]
    ldp x26, x27, [x30, #208]
    ldp x28, x29, [x30, #224]
    ldr x30, [x30, #240]
    eret
.endm

.Lexc_lower_sync:
    SAVE_ALL
    mov x0, sp
    mov x1, #TRAP_LOWER_SYNC
    bl trap_dispatch
    RESUME_ALL x0

.Lexc_current_sync:
    SAVE_ALL
    mov x0, sp
    mov x1, #TRAP_CURRENT_SYNC
    bl trap_dispatch
    RESUME_ALL x0

.Lexc_irq:
    SAVE_ALL
    mov x0, sp
    mov x1, #TRAP_IRQ
    bl trap_dispatch
    RESUME_ALL x0

// Unexpected exception (FIQ/SError/SP0 slot): report from Rust and halt.
.Ltrap_unexpected:
    SAVE_ALL
    mov x0, sp
    mov x1, #99
    bl trap_unexpected
1:  wfe
    b 1b

// Resume a fresh process context; never returns.
// Called by switch_to_user() (kernel/main.c port).
.global eret_frame
eret_frame:                     // x0 = &StackFrame to load
    RESUME_ALL x0
