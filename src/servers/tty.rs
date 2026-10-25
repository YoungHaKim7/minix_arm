//! Rust port of the TTY/console server (`minix/minix/drivers/tty/` and
//! `kernel/arch/*/console.c`): a character device server answering
//! CDEV_* requests.  Writes go to the PL011 UART in polled mode — the
//! interrupt-driven input side of the original is not converted yet.

use super::{receive, send};
use crate::console::putc;
use crate::minix::*;

pub extern "C" fn tty_main() -> ! {
    let mut m = Message::zeroed();
    loop {
        let r = unsafe { receive(ANY, &mut m) };
        if r != OK {
            continue;
        }
        let caller = m.m_source;
        let call = m.m_type;
        match call {
            CDEV_WRITE => {
                // CDEV_WRITE: byte count in m3i1, bytes in m3ca1
                // (the classic mess_3 console write of Tanenbaum's
                // textbook kernel).
                let count = unsafe { m.m_u.m_m3.m3i1 } as usize;
                let count = count.min(40);
                let bytes = unsafe { &m.m_u.m_m3.m3ca1[..count] };
                for &b in bytes {
                    if b == b'\n' {
                        putc(b'\r');
                    }
                    putc(b);
                }
                m = Message::zeroed();
                m.m_type = CDEV_WRITE;
                m.m_u.m_m3.m3i1 = count as i32;
                let _ = unsafe { send(caller, &mut m) };
            }
            CDEV_OPEN | CDEV_CLOSE => {
                m = Message::zeroed();
                m.m_type = call;
                unsafe { m.m_u.m_u64[0] = OK as u64 };
                let _ = unsafe { send(caller, &mut m) };
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
