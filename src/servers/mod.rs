//! User-space side of the port: the IPC trap wrappers (the equivalent of
//! `ipc_send`/`ipc_receive`/`ipc_sendrec` in `minix/minix/lib/` and the
//! libc syscall stubs) and the EL0 servers themselves.
//!
//! The servers are linked into the kernel image and run at EL0 under the
//! flat identity map; on real MINIX they are separate binaries started by
//! the boot monitor and VM.  The message flow — user -> PM/TTY -> kernel
//! -> user — is exactly the MINIX one.

pub mod init;
pub mod pm;
pub mod tty;

use crate::config::USER_STACK_SIZE;
use crate::minix::*;

/*===========================================================================*
 *                Process numbers of the boot services (com.h)               *
 *===========================================================================*/
pub const PM_PROC_NR: i32 = 0; // Process Manager
pub const TTY_PROC_NR: i32 = 1; // terminal driver slot (stands in for tty)
pub const INIT_PROC_NR: i32 = 2; // first user process

/// Endpoints of the servers, generation 0.
pub const PM_ENDPT: i32 = 0 + PM_PROC_NR;
pub const TTY_ENDPT: i32 = 0 + TTY_PROC_NR;

/*===========================================================================*
 *                IPC traps (ports the sys_ipc trap stubs)                   *
 *===========================================================================*/

/// `send()`: blocking send (libminix `ipc_send`).
pub unsafe fn send(dst: i32, m: *mut Message) -> i32 {
    ipc_trap(SEND as u64, dst, m)
}

/// `receive()`: blocking receive (libminix `ipc_receive`).
pub unsafe fn receive(src: i32, m: *mut Message) -> i32 {
    ipc_trap(RECEIVE as u64, src, m)
}

/// `sendrec()`: send + wait for reply (libminix `ipc_sendrec`).
pub unsafe fn sendrec(dst: i32, m: *mut Message) -> i32 {
    ipc_trap(SENDREC as u64, dst, m)
}

/// Kernel call from a system process (SYS_* via SYSTEM task).
pub unsafe fn kernel_call(m: *mut Message) -> i32 {
    let ret: u64;
    core::arch::asm!(
        "svc #1",
        inlateout("x0") 0u64 => ret,
        in("x1") m as usize as u64,
        options(nostack)
    );
    ret as i32
}

#[inline(always)]
unsafe fn ipc_trap(call: u64, srcdst: i32, m: *mut Message) -> i32 {
    let ret: u64;
    core::arch::asm!(
        "svc #0",
        inlateout("x0") call => ret,
        in("x1") (srcdst as i64) as u64,
        in("x2") m as usize as u64,
        options(nostack)
    );
    ret as i32
}

/*===========================================================================*
 *                Boot stacks for the EL0 processes                          *
 *===========================================================================*
 * VM assigns each process its memory in MINIX; in this port the stacks
 * are kernel statics under the identity map.
 */

#[repr(align(16))]
pub struct UserStack([u64; USER_STACK_SIZE / 8]);

pub static mut PM_STACK: UserStack = UserStack([0; USER_STACK_SIZE / 8]);
pub static mut TTY_STACK: UserStack = UserStack([0; USER_STACK_SIZE / 8]);
pub static mut INIT_STACK: UserStack = UserStack([0; USER_STACK_SIZE / 8]);

pub unsafe fn stack_top(s: *const UserStack) -> usize {
    s as usize + USER_STACK_SIZE
}

/// Format an unsigned number into buf, returning the written slice
/// (user-side replacement for printf, which lives in libc there).
pub fn fmt_u64(mut v: u64, buf: &mut [u8; 24]) -> &str {
    if v == 0 {
        buf[0] = b'0';
        return unsafe { core::str::from_utf8_unchecked(&buf[..1]) };
    }
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    unsafe { core::str::from_utf8_unchecked(&buf[i..]) }
}
