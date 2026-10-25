//! Rust port of the kernel-call dispatcher from
//! `minix/minix/kernel/system.c` (the SYSTEM task).  Privileged servers
//! (PM, drivers) issue kernel calls with `svc #1` and a message whose
//! m_type is KERNEL_CALL + subcall; user processes are denied, exactly as
//! the `s_trap_mask` / SYS_PROC privilege checks in the original do.

use crate::kernel::proc::{isokendpt, rts_isset, RTS_PROC_STOP, RTS_SENDING, RTS_SLOT_FREE};
use crate::kernel::sched::rts_setflags;
use crate::kernel::Kernel;
use crate::kprintf;
use crate::minix::{
    endpoint, Message, EBADCALL, ECALLDENIED, EDEADSRCDST, OK, SYS_ABORT, SYS_EXIT,
    SYS_SETALARM, SYS_TIMES,
};

/// `do_kernel_call()`: dispatch one kernel call message.  The message is
/// read from and written back to the caller's buffer in place.
pub unsafe fn do_kernel_call(k: &mut Kernel, caller: usize, m_ptr: *mut Message) -> i32 {
    // Only system processes may issue kernel calls (priv s_flags check).
    if !k.proc[caller].p_sys_proc {
        return ECALLDENIED;
    }
    let m = &mut *m_ptr;
    match m.m_type {
        SYS_SETALARM => {
            // sys_setalarm(): arm a timer for the caller, expiring after
            // the delay (ticks) carried in the first payload word.
            let delay = unsafe { m.m_u.m_u64[0] };
            if delay == 0 {
                k.proc[caller].p_alarm = None;
            } else {
                k.proc[caller].p_alarm = Some(k.realtime + delay);
            }
            OK
        }
        SYS_TIMES => crate::kernel::clock::sys_times(k, m),
        SYS_EXIT => {
            // sys_exit(): terminate the process whose endpoint is in the
            // first payload word (PM kills user processes this way).
            let target_e = unsafe { m.m_u.m_u64[0] } as i32;
            sys_exit(k, target_e)
        }
        SYS_ABORT => {
            kprintf!(
                "kernel: sys_abort from {}, halting\r\n",
                k.proc[caller].p_name
            );
            loop {
                core::hint::spin_loop();
            }
        }
        _ => EBADCALL,
    }
}

/// `sys_exit()`: stop a process and free its slot.  The next reuse of the
/// slot gets a new endpoint generation, so stale IPC to the dead process
/// fails with EDEADSRCDST.
pub unsafe fn sys_exit(k: &mut Kernel, target_e: i32) -> i32 {
    let target = match isokendpt(k, target_e) {
        Some(t) => t,
        None => return EDEADSRCDST,
    };

    kprintf!(
        "kernel: {} terminated by sys_exit (pid {})\r\n",
        k.proc[target].p_name,
        k.proc[target].p_nr
    );

    // If it is blocked sending, unlink it from the destination's queue.
    if rts_isset(&k.proc[target], RTS_SENDING) {
        if let Some(dst) = isokendpt(k, k.proc[target].p_sendto_e) {
            let mut link: *mut Option<usize> = &mut k.proc[dst].p_caller_q;
            while let Some(i) = *link {
                if i == target {
                    *link = k.proc[i].p_q_link;
                    break;
                }
                link = &mut k.proc[i].p_q_link;
            }
        }
    }

    // Clear pending state and mark the slot free.
    k.proc[target].p_q_link = None;
    k.proc[target].p_caller_q = None;
    k.proc[target].p_alarm = None;
    k.proc[target].p_notify_pending = 0;
    k.proc[target].p_misc_flags = 0;
    rts_setflags(k, target, RTS_PROC_STOP | RTS_SLOT_FREE);
    if k.proc_ptr == Some(target) {
        k.proc_ptr = None;
    }

    // Bump the endpoint generation so the slot can be reused safely.
    let nr = k.proc[target].p_nr;
    let generation = endpoint::endpoint_generation(k.proc[target].p_endpoint) + 1;
    k.proc[target].p_endpoint = endpoint::endpoint(generation, nr);
    OK
}
