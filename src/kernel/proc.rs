//! Rust port of `minix/minix/kernel/proc.h` (process table entry, runtime
//! flags) and `proc_init()`.  Where the C uses linked-list pointers
//! (`p_nextready`, `p_caller_q`, `p_q_link`), this port stores slot
//! indices to stay pointer-aliasing-safe in Rust.

use crate::arch::StackFrame;
use crate::minix::{Message, endpoint, endpoint_slot, NONE};

/*===========================================================================*
 *                Runtime flags: a process is runnable iff zero              *
 *===========================================================================*/
pub const RTS_SLOT_FREE: u32 = 0x001; // process slot is free
pub const RTS_PROC_STOP: u32 = 0x002; // process has been stopped
pub const RTS_SENDING: u32 = 0x004; // process blocked trying to send
pub const RTS_RECEIVING: u32 = 0x008; // process blocked trying to receive
pub const RTS_SIGNALED: u32 = 0x010; // set when new kernel signal arrives
pub const RTS_SIG_PENDING: u32 = 0x020; // unready while signal being processed
pub const RTS_P_STOP: u32 = 0x040; // set when process is being traced
pub const RTS_NO_PRIV: u32 = 0x080; // keep forked system process from running
pub const RTS_NO_ENDPOINT: u32 = 0x100; // process cannot send or receive
pub const RTS_VMINHIBIT: u32 = 0x200; // not scheduled until pagetable set
pub const RTS_PAGEFAULT: u32 = 0x400; // process has unhandled pagefault
pub const RTS_VMREQUEST: u32 = 0x800; // originator of vm memory request
pub const RTS_VMREQTARGET: u32 = 0x1000; // target of vm memory request
pub const RTS_PREEMPTED: u32 = 0x4000; // preempted by a higher priority process
pub const RTS_NO_QUANTUM: u32 = 0x8000; // ran out of quantum, requeue at tail
pub const RTS_BOOTINHIBIT: u32 = 0x10000; // not ready until VM has made it

/*===========================================================================*
 *                Misc flags (subset exercised by this port)                  *
 *===========================================================================*/
pub const MF_REPLY_PEND: u32 = 0x001; // reply to IPC request is pending
pub const MF_DELIVERMSG: u32 = 0x040; // copy message for him before running

/*===========================================================================*
 *                The process table entry (`struct proc`)                     *
 *===========================================================================*/

#[derive(Clone, Copy)]
pub struct Proc {
    /// `p_reg`: process' registers saved in stack frame.
    pub p_reg: StackFrame,
    /// `p_nr`: number of this process (slot number, may be negative).
    pub p_nr: i32,
    /// `p_rts_flags`: process is runnable only if zero.
    pub p_rts_flags: u32,
    /// `p_misc_flags`: flags that do not suspend the process.
    pub p_misc_flags: u32,
    /// `p_priority`: current scheduling priority (queue index).
    pub p_priority: usize,
    /// `p_cpu_time_left`: time left to use the cpu, in ticks.
    pub p_cpu_time_left: u64,
    /// `p_quantum_size_ms`-alike: assigned quantum in ticks.
    pub p_quantum: u64,

    /// `p_nextready`: next process in its run queue.
    pub p_nextready: Option<usize>,
    /// `p_caller_q`: head of list of processes wishing to send to us.
    pub p_caller_q: Option<usize>,
    /// `p_q_link`: link to next process wishing to send.
    pub p_q_link: Option<usize>,
    /// `p_getfrom_e`: from whom does the process want to receive?
    pub p_getfrom_e: i32,
    /// `p_sendto_e`: to whom does the process want to send?
    pub p_sendto_e: i32,

    /// `p_name`.
    pub p_name: &'static str,
    /// `p_endpoint`: endpoint number, generation-aware.
    pub p_endpoint: i32,

    /// `p_sendmsg`: message from this process if SENDING.
    pub p_sendmsg: Message,
    /// `p_delivermsg`: message for this process when it unblocks.
    pub p_delivermsg: Message,
    /// `p_delivermsg_vir`: virtual address this proc wants message at.
    pub p_delivermsg_vir: *mut Message,

    /// `p_defer`-alike: remembered do_ipc() arguments, used to run the
    /// RECEIVE half of a SENDREC that first blocked on SEND.
    pub p_ipc_srcdst: i32,
    pub p_ipc_msgptr: *mut Message,

    /// `s_notify_pending` bitmap (priv.h): pending notifications, one bit
    /// per process table slot.
    pub p_notify_pending: u128,

    /// Alarm timer set through sys_setalarm (clock.c watchdog list).
    pub p_alarm: Option<u64>,

    /// privilege flags subset: system processes may make kernel calls
    /// (`s_flags & SYS_PROC`) and are not preempted by the scheduler
    /// (`s_flags & PREEMPTIBLE` governs the opposite).
    pub p_sys_proc: bool,
    pub p_preemptible: bool,
}

pub const EMPTY_PROC: Proc = Proc {
    p_reg: StackFrame::ZERO,
    p_nr: 0,
    p_rts_flags: RTS_SLOT_FREE,
    p_misc_flags: 0,
    p_priority: 0,
    p_cpu_time_left: 0,
    p_quantum: 0,
    p_nextready: None,
    p_caller_q: None,
    p_q_link: None,
    p_getfrom_e: NONE,
    p_sendto_e: NONE,
    p_name: "",
    p_endpoint: 0,
    p_sendmsg: Message::zeroed(),
    p_delivermsg: Message::zeroed(),
    p_delivermsg_vir: core::ptr::null_mut(),
    p_ipc_srcdst: NONE,
    p_ipc_msgptr: core::ptr::null_mut(),
    p_notify_pending: 0,
    p_alarm: None,
    p_sys_proc: false,
    p_preemptible: false,
};

/// `proc_is_runnable(p)`: runnable iff flags are zero.
pub fn proc_is_runnable(p: &Proc) -> bool {
    p.p_rts_flags == 0
}

/// `RTS_ISSET(rp, f)`.
pub fn rts_isset(p: &Proc, f: u32) -> bool {
    (p.p_rts_flags & f) == f
}

/// `P_BLOCKEDON(p)`: on which endpoint is a process blocked?  Check
/// SENDING first, then RECEIVING, as in proc.h.
pub fn blocked_on(p: &Proc) -> i32 {
    if rts_isset(p, RTS_SENDING) {
        p.p_sendto_e
    } else if rts_isset(p, RTS_RECEIVING) {
        p.p_getfrom_e
    } else {
        NONE
    }
}

/// `isokendpt(e, &p)` (const.h/proc.c): endpoint to slot with generation
/// check.  Returns the table index, or None.
pub unsafe fn isokendpt(k: &crate::kernel::Kernel, e: i32) -> Option<usize> {
    let nr = endpoint_slot(e);
    let idx = (crate::config::NR_TASKS as i32 + nr) as usize;
    if !crate::kernel::isokprocslot(idx) {
        return None;
    }
    let p = &k.proc[idx];
    if p.p_endpoint != e || rts_isset(p, RTS_SLOT_FREE) || rts_isset(p, RTS_NO_ENDPOINT) {
        return None;
    }
    Some(idx)
}

/// `proc_init()` (proc.c): set up the process table.
pub unsafe fn proc_init() {
    let k = crate::kernel::kernel();
    for i in 0..(*k).proc.len() {
        let nr = i as i32 - crate::config::NR_TASKS as i32;
        (*k).proc[i].p_nr = nr;
        // Generation 0 endpoints: endpoint number == slot number.
        (*k).proc[i].p_endpoint = endpoint::endpoint(0, nr);
    }
}

/// Park a process as blocked in RECEIVE from ANY (used for the CLOCK
/// task, which never receives in this port).
pub unsafe fn set_blocked_receiving(k: &mut crate::kernel::Kernel, i: usize) {
    k.proc[i].p_getfrom_e = crate::minix::ANY;
    super::sched::rts_set(k, i, RTS_RECEIVING);
}

/// Build one process slot and make it runnable — the boot-time part of
/// `main.c`/`table.c` that installs the system image (IDLE task, the
/// servers, init) into the process table.
pub unsafe fn create_process(
    k: &mut crate::kernel::Kernel,
    nr: i32,
    name: &'static str,
    entry: usize,
    sp: usize,
    pstate: u64,
    priority: usize,
    sys_proc: bool,
    preemptible: bool,
    quantum: u64,
) -> usize {
    let i = k.proc_addr(nr);
    let p = &mut k.proc[i];
    *p = EMPTY_PROC;
    p.p_nr = nr;
    p.p_endpoint = endpoint::endpoint(0, nr);
    p.p_name = name;
    p.p_reg = StackFrame::ZERO;
    p.p_reg.pc = entry as u64;
    p.p_reg.sp = sp as u64;
    p.p_reg.pstate = pstate;
    p.p_priority = priority;
    p.p_quantum = quantum;
    p.p_cpu_time_left = quantum;
    p.p_sys_proc = sys_proc;
    p.p_preemptible = preemptible;
    // Slot starts free; clearing the flag enqueues the process
    // (RTS_UNSET semantics).
    super::sched::rts_unset(k, i, RTS_SLOT_FREE);
    i
}
