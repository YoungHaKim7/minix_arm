ENTRY(_start)

/* QEMU virt RAM starts at 0x40000000.  The image is split into 2MB
 * blocks so the stage-1 page tables can give each zone its own
 * permissions:
 *   0x40000000 .. 0x401FFFFF  kernel text/rodata: read-only, executable
 *                             at EL0 and EL1 (user servers run here)
 *   0x40200000 .. 0x403FFFFF  kernel data/bss + kernel stacks: RW at EL1
 *   0x40400000 ..             user stacks and user memory: RW at EL0+EL1
 * (EL1 may only execute from pages that are not writable at EL0.)
 */
SECTIONS
{
    . = 0x40080000;

    .text : ALIGN(4096) {
        KEEP(*(.text.boot))
        *(.text .text.*)
    }

    .rodata : ALIGN(4096) {
        *(.rodata .rodata.*)
    }

    /* Kernel data gets its own 2MB block (EL1-only). */
    . = ALIGN(2097152);
    .data : ALIGN(4096) {
        *(.data .data.*)
    }

    .bss (NOLOAD) : ALIGN(4096) {
        *(.bss .bss.*)
        *(COMMON)

        . = ALIGN(16);
        . += 16384;
        _stack_top = .;
    }

    /* User memory gets its own 2MB block (EL0+EL1 RW). */
    . = ALIGN(2097152);
    .user : ALIGN(4096) {
        __user_start = .;
        *(.user .user.*)
        __user_end = .;
    }

    /DISCARD/ : {
        *(.eh_frame .eh_frame.*)
    }
}
