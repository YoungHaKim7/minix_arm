//! Rust port of the configuration constants from
//! `minix/minix/include/minix/config.h` and `kernel/config.h`.

/// `NR_PROCS`: number of slots for user/system processes.
pub const NR_PROCS: usize = 64;

/// `NR_TASKS`: number of kernel task slots (IDLE, CLOCK, SYSTEM, KERNEL).
pub const NR_TASKS: usize = 4;

/// Total process table size: `NR_TASKS + NR_PROCS` slots.
pub const NR_TASKS_PLUS_PROCS: usize = NR_TASKS + NR_PROCS;

/*===========================================================================*
 *                Scheduling queues (config.h)                               *
 *===========================================================================*/
/// `NR_SCHED_QUEUES`: MUST equal minimum priority + 1.
pub const NR_SCHED_QUEUES: usize = 16;

/// `MAX_USER_Q`: highest priority for user processes.
pub const MAX_USER_Q: usize = 0;

/// `USER_Q`: default priority for new user processes.
pub const USER_Q: usize = (MIN_USER_Q - MAX_USER_Q) / 2 + MAX_USER_Q;

/// `MIN_USER_Q`: minimum priority for user processes.
pub const MIN_USER_Q: usize = NR_SCHED_QUEUES - 1;

/// `TASK_Q`: priority of kernel tasks.
#[allow(dead_code)]
pub const TASK_Q: usize = 0;

/// `IDLE_Q`: priority of the IDLE process; always the last queue.
pub const IDLE_Q: usize = NR_SCHED_QUEUES - 1;

/// Priority assigned to system services (PM, TTY) at boot.
pub const SERVER_Q: usize = 2;

/*===========================================================================*
 *                Clock (kernel/config.h, clock.c)                           *
 *===========================================================================*/
/// `SYSTEM_FREQ`-derived tick rate: 100 Hz, as in MINIX.
pub const SYSTEM_HZ: u64 = 100;

/// Time quantum given to user processes, in ticks (clock.c: 200 ms).
pub const USER_QUANTUM_TICKS: u64 = 20;

/// `PROC_NAME_LEN` (kernel/proc.h via com.h).
#[allow(dead_code)]
pub const PROC_NAME_LEN: usize = 16;

/// Size of the per-process kernel (exception) stack.
pub const KERNEL_STACK_SIZE: usize = 4096;

/// Size of the per-process user (EL0) stack.
pub const USER_STACK_SIZE: usize = 8192;
