//! Rust port of the Process Manager server (`minix/minix/servers/pm/`):
//! the user-space server that owns process lifecycle.  A tiny slice of
//! its main loop is converted: PM_GETPID, itimer-based alarms
//! (PM_ITIMER -> kernel sys_setalarm) and PM_EXIT (-> kernel sys_exit),
//! each answered with the standard MINIX request/reply message exchange.

use super::{kernel_call, notify, receive, send};
use crate::minix::*;

pub extern "C" fn pm_main() -> ! {
    let mut m = Message::zeroed();
    // Simplified pm/timers.c: the endpoint whose itimer is running.
    let mut alarm_owner: i32 = NONE;
    loop {
        // The PM main loop (servers/pm/main.c): receive ANY, execute,
        // reply.
        let r = unsafe { receive(ANY, &mut m) };
        if r != OK {
            continue;
        }
        let caller = m.m_source;
        let call = m.m_type;

        // The CLOCK task notifies us when a timer expires (sys_setalarm
        // arms the timer on its caller, i.e. us); forward the wakeup to
        // the process that asked for the itimer (pm/timers.c role).
        if is_notify(call) && m.m_source == CLOCK && alarm_owner != NONE {
            let target = alarm_owner;
            alarm_owner = NONE;
            let mut n = Message::zeroed();
            let _ = unsafe { notify(target, &mut n) };
            continue;
        }

        match call {
            PM_GETPID => {
                // The PM tracks pids in mproc[]; with the flat port the
                // process number doubles as the pid.
                let pid = endpoint_slot(caller);
                m = Message::zeroed();
                m.m_type = PM_GETPID;
                m.m_u.m_m1.m1i1 = pid;
                let _ = unsafe { send(caller, &mut m) };
            }
            PM_ITIMER => {
                // alarm()/setitimer(): ask the kernel to arm the timer on
                // our behalf; remember who to wake when it fires.
                alarm_owner = caller;
                let delay = unsafe { m.m_u.m_u64[0] };
                let mut km = Message::zeroed();
                km.m_type = SYS_SETALARM;
                unsafe { km.m_u.m_u64[0] = delay };
                let kr = unsafe { kernel_call(&mut km) };
                m = Message::zeroed();
                m.m_type = PM_ITIMER;
                unsafe { m.m_u.m_u64[0] = kr as u64 };
                let _ = unsafe { send(caller, &mut m) };
            }
            PM_EXIT => {
                // Reply first so the caller's sendrec completes, then have
                // the kernel free its slot (sys_exit).
                m = Message::zeroed();
                m.m_type = PM_EXIT;
                unsafe { m.m_u.m_u64[0] = OK as u64 };
                let _ = unsafe { send(caller, &mut m) };
                let mut km = Message::zeroed();
                km.m_type = SYS_EXIT;
                unsafe { km.m_u.m_u64[0] = caller as u64 };
                let _ = unsafe { kernel_call(&mut km) };
            }
            _ => {
                m = Message::zeroed();
                m.m_type = call;
                unsafe { m.m_u.m_u64[0] = EINVAL as u64 };
                let _ = unsafe { send(caller, &mut m) };
            }
        }
    }
}
