//! MINIX kernel, converted to Rust for aarch64 — the port of
//! `minix/minix/kernel/main.c` (`kmain` / `main`): bring up the machine,
//! announce ourselves, build the boot process table and start the first
//! process.  Control never returns here after `switch_to_user()`.
//!
//! Original module map (file -> this port):
//!   kernel/proc.h, proc.c      -> kernel/proc.rs, kernel/ipc.rs, kernel/sched.rs
//!   kernel/clock.c             -> kernel/clock.rs (+ arch.rs timer)
//!   kernel/system.c            -> kernel/system.rs
//!   kernel/arch/*              -> arch.rs + asm/boot.S
//!   include/minix/*.h          -> minix/ module
//!   servers/pm, drivers/tty, servers/init -> servers/ (running at EL0)

#![no_std]
#![no_main]
// This is a direct C port: unsafe fns keep the C convention of an
// implicitly unsafe body.
#![allow(unsafe_op_in_unsafe_fn)]
// The port keeps the full constant surfaces of the converted headers
// (RTS_*/MF_*/SYS_*/...) even where this kernel does not use every
// member yet.
#![allow(dead_code)]

mod arch;
mod config;
mod console;
mod kernel;
mod minix;
mod servers;

use core::panic::PanicInfo;

// The aarch64 boot code and exception vectors (kernel/arch/* port).
core::arch::global_asm!(include_str!("../asm/boot.S"));

use arch::{idle_entry_addr, SPSR_KERNEL, SPSR_USER};
use config::{IDLE_Q, SERVER_Q, USER_Q, USER_QUANTUM_TICKS};
use kernel::proc::create_process;
use minix::{CLOCK, IDLE, SYSTEM};

/// The boot image (main.c + table.c): which processes exist at startup.
unsafe fn build_proc_table() {
    let k = &mut *kernel::kernel();

    // Kernel tasks: negative slots, like the IDLE task in proc.c.
    create_process(
        k,
        IDLE,
        "IDLE",
        idle_entry_addr(),
        0,
        SPSR_KERNEL,
        IDLE_Q,
        true,
        false,
        0,
    );

    // CLOCK task (kernel/clock.c): this port services ticks inside the
    // interrupt handler, so the task itself stays blocked in RECEIVE
    // forever - like the original waiting for its HARD_INT message - but
    // its slot must exist so the CLOCK endpoint is valid for
    // notifications.
    let clock_idx = create_process(
        k,
        CLOCK,
        "CLOCK",
        idle_entry_addr(),
        0,
        SPSR_KERNEL,
        config::TASK_Q,
        true,
        false,
        0,
    );
    kernel::proc::set_blocked_receiving(k, clock_idx);

    // System servers at EL0: the Process Manager and the TTY driver.
    create_process(
        k,
        servers::PM_PROC_NR,
        "PM",
        (servers::pm::pm_main as extern "C" fn() -> !) as usize,
        servers::stack_top(core::ptr::addr_of!(servers::PM_STACK)),
        SPSR_USER,
        SERVER_Q,
        true,
        true,
        USER_QUANTUM_TICKS,
    );
    create_process(
        k,
        servers::TTY_PROC_NR,
        "TTY",
        (servers::tty::tty_main as extern "C" fn() -> !) as usize,
        servers::stack_top(core::ptr::addr_of!(servers::TTY_STACK)),
        SPSR_USER,
        SERVER_Q + 1,
        true,
        true,
        USER_QUANTUM_TICKS,
    );

    // First user process.
    create_process(
        k,
        servers::INIT_PROC_NR,
        "init",
        (servers::init::init_main as extern "C" fn() -> !) as usize,
        servers::stack_top(core::ptr::addr_of!(servers::INIT_STACK)),
        SPSR_USER,
        USER_Q,
        false,
        true,
        USER_QUANTUM_TICKS,
    );
}

/// Ports `announce()` from kernel/main.c: display the startup banner.
fn announce() {
    kprintf!("MINIX 3.4.0 (Rust port to aarch64/QEMU-virt)\r\n");
    kprintf!("Multiserver microkernel: {}, {}, {} tasks ready\r\n", IDLE, CLOCK, SYSTEM);
    kprintf!("Kernel: proc/sched/ipc/clock/system converted from C\r\n");
    kprintf!("Servers at EL0: PM, TTY; userland: init\r\n\r\n");
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_main() -> ! {
    console::init();

    unsafe {
        // cstart(): catch machine-dependent initialization.
        arch::early_init();

        // Initialize the process table.
        kernel::proc::proc_init();

        announce();

        // Install the boot image and start the ball rolling.
        build_proc_table();
        arch::timer_init();
        kernel::sched::switch_to_user(); // never returns
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    kprintf!("\r\nKERNEL PANIC: {}\r\n", info);
    loop {
        core::hint::spin_loop();
    }
}
