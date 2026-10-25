//! Rust port of the MINIX kernel (`minix/minix/kernel/`).
//!
//! The kernel state below corresponds to the globals in `kernel/glo.h`
//! and `kernel/proc.h`: the process table, the per-queue run heads/tails
//! and the currently running process.

pub mod clock;
pub mod ipc;
pub mod proc;
pub mod sched;
pub mod system;
pub mod trap;

use crate::arch::StackFrame;
use crate::config::{
    KERNEL_STACK_SIZE, NR_PROCS, NR_SCHED_QUEUES, NR_TASKS, NR_TASKS_PLUS_PROCS,
};
use proc::Proc;

/// Everything `kernel/glo.h` keeps in globals, plus the process table and
/// per-process kernel stacks.  Single core only: interrupt handlers run
/// with interrupts masked, so no locking is needed (as in UP MINIX).
pub struct Kernel {
    /// `proc[]`: the process table (proc.h).
    pub proc: [Proc; NR_TASKS_PLUS_PROCS],

    /// `rdy_head[]` / `rdy_tail[]`: run queues per priority (proc.c).
    pub rdy_head: [Option<usize>; NR_SCHED_QUEUES],
    pub rdy_tail: [Option<usize>; NR_SCHED_QUEUES],

    /// `proc_ptr`: slot index of the running process.
    pub proc_ptr: Option<usize>,

    /// `bill_ptr`: who to bill for system time (pick_proc sets it).
    pub bill_ptr: Option<usize>,

    /// `realtime`: uptime in clock ticks (kernel clock.c).
    pub realtime: u64,

    /// Per-process kernel (exception) stacks; SP_EL1 points here while a
    /// process runs, so its saved context always sits at the top.
    kstacks: [KStack; NR_TASKS_PLUS_PROCS],
}

#[repr(C, align(4096))]
#[derive(Clone, Copy)]
struct KStack([u8; KERNEL_STACK_SIZE]);

pub static mut KERNEL: Kernel = Kernel {
    proc: [proc::EMPTY_PROC; NR_TASKS_PLUS_PROCS],
    rdy_head: [None; NR_SCHED_QUEUES],
    rdy_tail: [None; NR_SCHED_QUEUES],
    proc_ptr: None,
    bill_ptr: None,
    realtime: 0,
    kstacks: [KStack([0; KERNEL_STACK_SIZE]); NR_TASKS_PLUS_PROCS],
};

/// Access the kernel state as a raw pointer; bare-metal replacement for
/// the C file-scope globals.
#[allow(static_mut_refs)]
pub unsafe fn kernel() -> *mut Kernel {
    core::ptr::addr_of_mut!(KERNEL)
}

impl Kernel {
    /// `proc_addr(n)`: index of process number `n` in the table.
    pub const fn proc_addr(&self, nr: i32) -> usize {
        (NR_TASKS as i32 + nr) as usize
    }

    /// Top of the kernel stack of process slot `i`; its saved context
    /// lives just below it.
    pub fn kstack_frame(&mut self, i: usize) -> *mut StackFrame {
        let top = self.kstacks[i].0.as_ptr() as usize + KERNEL_STACK_SIZE;
        (top - core::mem::size_of::<StackFrame>()) as *mut StackFrame
    }
}

/// `NR_TASKS + NR_PROCS` bound check used everywhere (`isokprocn`).
pub const fn isokprocslot(idx: usize) -> bool {
    idx < NR_TASKS + NR_PROCS
}
