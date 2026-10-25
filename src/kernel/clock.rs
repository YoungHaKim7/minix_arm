//! Rust port of `minix/minix/kernel/clock.c` and `watchdog.c`: the timer
//! interrupt handler that keeps the uptime, expires alarms (sending
//! CLOCK notifications) and charges time quanta, plus `proc_no_time()`
//! from proc.c for quantum expiry.

use crate::arch::{gic_ack, gic_eoi, timer_rearm, IRQ_TIMER};
use crate::config::USER_QUANTUM_TICKS;
use crate::kernel::ipc::mini_notify;
use crate::kernel::proc::RTS_NO_QUANTUM;
use crate::kernel::sched::{rts_set, rts_unset};
use crate::kernel::Kernel;
use crate::minix::CLOCK;

/// The timer interrupt: `clock_handler()` in clock.c.  Called from the
/// trap layer with interrupts masked, on every tick.
pub unsafe fn clock_interrupt(k: &mut Kernel) {
    let iar = gic_ack();
    let intid = iar & 0x3FF;
    if intid == IRQ_TIMER {
        timer_rearm();
        tick(k);
    }
    if intid != 1023 {
        gic_eoi(iar);
    }
}

/// One SYSTEM_HZ tick: advance the clock, fire expired alarms, and
/// enforce the running process's quantum.
unsafe fn tick(k: &mut Kernel) {
    k.realtime += 1;

    // Expire alarms (ports cause_alarm/watchdog chain): notify the
    // processes whose sys_setalarm timer ran out, as the CLOCK task
    // would in classic MINIX.
    for i in 0..k.proc.len() {
        if let Some(t) = k.proc[i].p_alarm {
            if k.realtime >= t {
                k.proc[i].p_alarm = None;
                let dst = k.proc[i].p_endpoint;
                mini_notify(k, CLOCK, dst);
            }
        }
    }

    // Charge time against the current process; on quantum expiry
    // requeue it at the tail of its queue and give it a fresh quantum
    // (proc_no_time() + the kernel-scheduler policy in sched servers).
    if let Some(cur) = k.proc_ptr {
        if k.proc[cur].p_preemptible {
            if k.proc[cur].p_cpu_time_left > 0 {
                k.proc[cur].p_cpu_time_left -= 1;
            }
            if k.proc[cur].p_cpu_time_left == 0 {
                k.proc[cur].p_cpu_time_left = USER_QUANTUM_TICKS;
                rts_set(k, cur, RTS_NO_QUANTUM); // dequeues
                rts_unset(k, cur, RTS_NO_QUANTUM); // requeues at tail
            }
        }
    }
}

/// `SYS_TIMES` support: fill the reply with the current uptime.
pub unsafe fn sys_times(k: &Kernel, m: &mut crate::minix::Message) -> i32 {
    unsafe { m.m_u.m_u64[0] = k.realtime };
    crate::minix::OK
}
