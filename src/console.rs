//! Console driver: PL011 UART on the QEMU "virt" machine, at 0x0900_0000.
//! This is the aarch64 counterpart of the MINIX `tty`/console layer
//! (kernel/arch/*/console.c) and provides the kernel's `printf`.

use core::fmt;

const UART0_BASE: usize = 0x0900_0000;
const UART_DR: usize = UART0_BASE + 0x00; // data register
const UART_FR: usize = UART0_BASE + 0x18; // flag register
const UART_FR_TXFF: u32 = 1 << 5; // transmit FIFO full

/// QEMU's PL011 works out of reset; MINIX's console does the same
/// minimal bring-up (`console_init` in the original).
pub fn init() {
    // Nothing to configure on QEMU virt; the UART is ready.
}

pub fn putc(c: u8) {
    unsafe {
        while (core::ptr::read_volatile(UART_FR as *const u32) & UART_FR_TXFF) != 0 {
            core::hint::spin_loop();
        }
        core::ptr::write_volatile(UART_DR as *mut u8, c);
    }
}

/// `kputc` (kernel/utility.c): kernel message character output; a zero
/// byte marks END_OF_KMESS and is not printed.
#[allow(dead_code)]
pub fn kputc(c: u8) {
    if c != 0 {
        putc(c);
    }
}

pub struct Console;

impl fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            if b == b'\n' {
                putc(b'\r');
            }
            putc(b);
        }
        Ok(())
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    let _ = Console.write_fmt(args);
}

/// Kernel `printf`. Not interrupt-safe against concurrent use; the kernel
/// is single-core and runs handlers with interrupts masked, as in MINIX.
#[macro_export]
macro_rules! kprintf {
    ($($arg:tt)*) => ($crate::console::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! kputs {
    ($($arg:tt)*) => ({
        $crate::kprintf!("{}\r\n", format_args!($($arg)*));
    });
}
