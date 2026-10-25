ENTRY(_start)

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

    /DISCARD/ : {
        *(.eh_frame .eh_frame.*)
    }
}
