//! Rust port of the scheduling queue management from
//! `minix/minix/kernel/proc.c`: `enqueue()`, `enqueue_head()`,
//! `dequeue()`, `pick_proc()` and the RTS_SET / RTS_UNSET / RTS_SETFLAGS
//! macros from proc.h.  The scheduling policy: sixteen priority queues,
//! a process is runnable iff its RTS flags are zero, and the first
//! process of the first non-empty queue runs.

use crate::config::NR_SCHED_QUEUES;
use crate::kernel::{kernel, Kernel};
use crate::kprintf;

use super::proc::{proc_is_runnable, RTS_PREEMPTED};

/// `enqueue()`: add a runnable process to the tail of its priority queue.
pub unsafe fn enqueue(k: &mut Kernel, rp: usize) {
    let q = k.proc[rp].p_priority;

    if !proc_is_runnable(&k.proc[rp]) {
        kprintf!("enqueue: {} not runnable\r\n", k.proc[rp].p_name);
    }

    k.proc[rp].p_nextready = None;
    match k.rdy_tail[q] {
        None => {
            // add to empty queue
            k.rdy_head[q] = Some(rp);
            k.rdy_tail[q] = Some(rp);
        }
        Some(tail) => {
            // add to tail of queue
            k.proc[tail].p_nextready = Some(rp);
            k.rdy_tail[q] = Some(rp);
        }
    }

    // Enqueueing a process with a higher priority than the current one
    // preempts it, if the current process is preemptible (enqueue()).
    if let Some(cur) = k.proc_ptr {
        if cur != rp && k.proc[cur].p_priority > q && k.proc[cur].p_preemptible {
            rts_set(k, cur, RTS_PREEMPTED); // calls dequeue()
        }
    }
}

/// `dequeue()`: remove a process from the scheduling queues.  A process
/// must be removed when it blocks; if it was not linked (e.g. it was
/// running), this is a no-op.
pub unsafe fn dequeue(k: &mut Kernel, rp: usize) {
    for q in 0..NR_SCHED_QUEUES {
        let mut prev: Option<usize> = None;
        let mut cur = k.rdy_head[q];
        while let Some(i) = cur {
            let next = k.proc[i].p_nextready;
            if i == rp {
                match prev {
                    Some(p) => k.proc[p].p_nextready = next,
                    None => k.rdy_head[q] = next,
                }
                if next.is_none() {
                    k.rdy_tail[q] = prev;
                }
                k.proc[rp].p_nextready = None;
                return;
            }
            prev = cur;
            cur = next;
        }
    }
}

/// `enqueue_head()`: put a process at the front of its run queue, used
/// when a preempted process is put back so it is treated fairly.
pub unsafe fn enqueue_head(k: &mut Kernel, rp: usize) {
    let q = k.proc[rp].p_priority;
    k.proc[rp].p_nextready = k.rdy_head[q];
    if k.rdy_head[q].is_none() {
        k.rdy_tail[q] = Some(rp);
    }
    k.rdy_head[q] = Some(rp);
}

/// `pick_proc()`: decide who to run now.  The first process in the first
/// non-empty queue wins; IDLE (last queue) is the fallback.  Also updates
/// `bill_ptr`.
pub unsafe fn pick_proc(k: &mut Kernel) -> Option<usize> {
    for q in 0..NR_SCHED_QUEUES {
        if let Some(rp) = k.rdy_head[q] {
            k.bill_ptr = Some(rp);
            return Some(rp);
        }
    }
    None
}

/// `RTS_SET(rp, f)`: set flag and dequeue if the process was runnable.
pub unsafe fn rts_set(k: &mut Kernel, rp: usize, f: u32) {
    let was = k.proc[rp].p_rts_flags;
    k.proc[rp].p_rts_flags |= f;
    if proc_is_runnable_flags(was) && !proc_is_runnable(&k.proc[rp]) {
        dequeue(k, rp);
    }
}

/// `RTS_UNSET(rp, f)`: clear flag and enqueue if the process is now
/// runnable.
pub unsafe fn rts_unset(k: &mut Kernel, rp: usize, f: u32) {
    let was = k.proc[rp].p_rts_flags;
    k.proc[rp].p_rts_flags &= !f;
    if !proc_is_runnable_flags(was) && proc_is_runnable(&k.proc[rp]) {
        enqueue(k, rp);
    }
}

/// `RTS_SETFLAGS(rp, f)`: set flags to this value.
pub unsafe fn rts_setflags(k: &mut Kernel, rp: usize, f: u32) {
    if proc_is_runnable(&k.proc[rp]) && f != 0 {
        dequeue(k, rp);
    }
    k.proc[rp].p_rts_flags = f;
}

fn proc_is_runnable_flags(flags: u32) -> bool {
    flags == 0
}

/*===========================================================================*
 *                Restart: pick a process and hand back its context          *
 *===========================================================================*/

/// The trap entry saved the interrupted context at the top of the current
/// process's kernel stack; after a handler ran, pick a process to run and
/// copy its saved `p_reg` back to the top of its kernel stack.  This is
/// the ports of `restart()` / the tail of exception handling.
pub unsafe fn schedule_frame(k: &mut Kernel) -> *mut crate::arch::StackFrame {
    // Ports the preempted-outgoing-process reconciliation from
    // switch_to_user()/restart() (proc.c:321-330): a process flagged
    // RTS_PREEMPTED while it was current has the flag cleared here, and
    // is put back at the front of its queue if it is runnable again
    // (front if it still has quantum, tail otherwise).
    if let Some(cur) = k.proc_ptr {
        if k.proc[cur].p_rts_flags & RTS_PREEMPTED != 0 {
            k.proc[cur].p_rts_flags &= !RTS_PREEMPTED;
            if proc_is_runnable(&k.proc[cur]) {
                if k.proc[cur].p_cpu_time_left > 0 {
                    enqueue_head(k, cur);
                } else {
                    enqueue(k, cur);
                }
            }
        }
    }
    let next = match pick_proc(k) {
        Some(n) => n,
        None => {
            kprintf!("KERNEL: no runnable process, halting\r\n");
            loop {
                core::hint::spin_loop();
            }
        }
    };
    k.proc_ptr = Some(next);
    let frame = k.kstack_frame(next);
    core::ptr::copy_nonoverlapping(&k.proc[next].p_reg as *const _, frame, 1);
    frame
}

/// `switch_to_user()` (main.c/proc.c): schedule the first process.  Never
/// returns.
pub unsafe fn switch_to_user() -> ! {
    let k = kernel();
    let frame = schedule_frame(&mut *k);
    crate::arch::switch_to_user(frame)
}
