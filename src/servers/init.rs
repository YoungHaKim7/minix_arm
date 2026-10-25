//! The first user process (`minix/minix/servers/init/` plus the
//! traditional hello-world test): everything it does goes through IPC —
//! console output via the TTY server, identity and timers via PM, alarms
//! through PM down to the kernel's sys_setalarm, exit through PM_EXIT.

use super::{fmt_u64, receive, sendrec, PM_ENDPT, TTY_ENDPT};
use crate::minix::*;

/// Write a string to the console through the TTY server, 40 bytes per
/// CDEV_WRITE message (the mess_3 payload size).
fn tty_write(s: &str) {
    let mut m;
    for chunk in s.as_bytes().chunks(40) {
        m = Message::zeroed();
        m.m_type = CDEV_WRITE;
        unsafe {
            m.m_u.m_m3.m3i1 = chunk.len() as i32;
            m.m_u.m_m3.m3ca1[..chunk.len()].copy_from_slice(chunk);
        }
        let _ = unsafe { sendrec(TTY_ENDPT, &mut m) };
    }
}

pub extern "C" fn init_main() -> ! {
    let mut m = Message::zeroed();
    let mut numbuf = [0u8; 24];

    tty_write("\r\n== userland: init started ==\r\n");
    tty_write("Hello from userland init via MINIX IPC!\r\n");

    // Ask PM who we are (sendrec: request + reply in one trap).
    m.m_type = PM_GETPID;
    let r = unsafe { sendrec(PM_ENDPT, &mut m) };
    if r == OK {
        let pid = unsafe { m.m_u.m_m1.m1i1 };
        tty_write("init: PM says my pid is ");
        tty_write(fmt_u64(pid as u64 as u64, &mut numbuf));
        tty_write("\r\n");
    }

    // Arm a 2 second alarm: POSIX alarm() -> PM (PM_ITIMER) -> kernel
    // sys_setalarm -> CLOCK notification back to us.
    m = Message::zeroed();
    m.m_type = PM_ITIMER;
    unsafe { m.m_u.m_u64[0] = 200 };
    let _ = unsafe { sendrec(PM_ENDPT, &mut m) };
    tty_write("init: sleeping 2s on a MINIX alarm...\r\n");

    // Block on receive(ANY) until the CLOCK notification arrives; the
    // notification payload carries the uptime.
    m = Message::zeroed();
    let r = unsafe { receive(ANY, &mut m) };
    if r == OK && is_notify(m.m_type) {
        let uptime = unsafe { m.m_u.m_u64[0] };
        let source = if m.m_source == CLOCK {
            "CLOCK"
        } else {
            "?"
        };
        tty_write("init: wakeup notification from ");
        tty_write(source);
        tty_write(" at uptime ");
        tty_write(fmt_u64(uptime, &mut numbuf));
        tty_write(" ticks\r\n");
    }

    tty_write("init: exiting via PM_EXIT -> sys_exit\r\n");

    // PM replies to our sendrec, then asks the kernel to terminate us;
    // we never run again after this trap.
    m = Message::zeroed();
    m.m_type = PM_EXIT;
    unsafe { m.m_u.m_u64[0] = 0 };
    let _ = unsafe { sendrec(PM_ENDPT, &mut m) };

    loop {
        core::hint::spin_loop();
    }
}
