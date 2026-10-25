#![no_std]
#![no_main]

use core::{
    arch::global_asm,
    panic::PanicInfo,
    ptr::{read_volatile, write_volatile},
};

global_asm!(include_str!("../asm/boot.S"));

const UART0_DR: *mut u8 = 0x0900_0000 as *mut u8;
const UART0_FR: *const u32 = 0x0900_0018 as *const u32;

const UART_FR_TXFF: u32 = 1 << 5;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_main() -> ! {
    let message = b"Hello World!\r\n";

    for &byte in message {
        unsafe {
            while (read_volatile(UART0_FR) & UART_FR_TXFF) != 0 {
                core::hint::spin_loop();
            }

            write_volatile(UART0_DR, byte);
        }
    }

    loop {
        core::hint::spin_loop();
    }
}
