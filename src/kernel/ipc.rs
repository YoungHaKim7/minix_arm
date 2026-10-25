//! Rust port of the synchronous IPC engine from `minix/minix/kernel/proc.c`:
//! `do_ipc()` / `do_sync_ipc()`, `mini_send()`, `mini_receive()`,
//! `mini_notify()`, `deadlock()` and the `WILLRECEIVE`/`CANRECEIVE`
//! macros.  This is the heart of the MINIX microkernel: all system
//! services are built on these blocking message primitives.
//!
//! aarch64 differences from the original:
//!  * processes enter through an `svc` trap (trap.rs) instead of
//!    sysenter/syscall, with (function, srcdst, message pointer) in
//!    x0/x1/x2 — the counterpart of `do_ipc(r1, r2, r3)`;
//!  * there is a single flat address space, so `copy_msg_from_user` is a
//!    plain copy;
//!  * SENDA (batched asynchronous send) is not converted yet.

use crate::kernel::proc::{
    blocked_on, isokendpt, rts_isset, MF_REPLY_PEND, RTS_RECEIVING, RTS_SENDING,
    RTS_SLOT_FREE,
};
use crate::kernel::sched::{rts_set, rts_unset};
use crate::kernel::Kernel;
use crate::kprintf;
use crate::minix::{
    endpoint_slot, Message, NOTIFY_MESSAGE, ANY, EBADCALL, EINVAL, ELOCKED, ENOTREADY, OK,
    RECEIVE, SEND, SENDNB, SENDREC,
};
use crate::config::NR_TASKS;

/// `WILLRECEIVE(from, dst)`: is `dst` blocked in RECEIVE and accepting a
/// message from endpoint `from`?
unsafe fn will_receive(k: &Kernel, from_e: i32, dst: usize) -> bool {
    let p = &k.proc[dst];
    rts_isset(p, RTS_RECEIVING) && (p.p_getfrom_e == ANY || p.p_getfrom_e == from_e)
}

/// Copy a message out of a process's address space (`copy_msg_from_user`).
/// With the flat identity map this is a direct copy.
unsafe fn copy_from_user(src: *const Message, dst: &mut Message) -> bool {
    if (src as usize) & 0x7 != 0 {
        return false; // EFAULT: message not aligned
    }
    *dst = core::ptr::read_volatile(src);
    true
}

unsafe fn copy_to_user(src: &Message, dst: *mut Message) -> bool {
    if (dst as usize) & 0x7 != 0 {
        return false;
    }
    core::ptr::write_volatile(dst, *src);
    true
}

/// Deliver `dst.p_delivermsg` to a process blocked in RECEIVE: copy the
/// message to its buffer, complete its IPC system call with OK and make
/// it runnable again.  Single core and immediate delivery mean the
/// MF_DELIVERMSG deferral of the original is not needed.
unsafe fn deliver_now(k: &mut Kernel, dst: usize) {
    let vir = k.proc[dst].p_delivermsg_vir;
    copy_to_user(&k.proc[dst].p_delivermsg, vir);
    k.proc[dst].p_reg.regs[0] = OK as u64; // syscall result
    k.proc[dst].p_misc_flags &= !MF_REPLY_PEND;
    rts_unset(k, dst, RTS_RECEIVING); // may enqueue()
}

/// The RECEIVE half of a SENDREC that first blocked while sending: run it
/// as soon as the request message has been delivered (the original
/// defers this through the restart path with `p_defer`).
unsafe fn ipc_continuation(k: &mut Kernel, sender: usize) {
    let srcdst = k.proc[sender].p_ipc_srcdst;
    let mptr = k.proc[sender].p_ipc_msgptr;
    mini_receive(k, sender, srcdst, mptr);
}

/*===========================================================================*
 *                mini_send                                                  *
 *===========================================================================*/

/// Send a message from `caller` to `dst_e`.  If the destination is blocked
/// waiting for this message, copy it over and unblock it; otherwise block
/// the caller on the destination's queue.
pub unsafe fn mini_send(
    k: &mut Kernel,
    caller: usize,
    dst_e: i32,
    m_ptr: *const Message,
    non_blocking: bool,
) -> i32 {
    let caller_e = k.proc[caller].p_endpoint;
    let dst = match isokendpt(k, dst_e) {
        Some(d) => d,
        None => return crate::minix::EDEADSRCDST,
    };

    if will_receive(k, caller_e, dst) {
        // Destination is indeed waiting for this message.
        if !copy_from_user(m_ptr, &mut k.proc[dst].p_delivermsg) {
            return crate::minix::EFAULT;
        }
        k.proc[dst].p_delivermsg.m_source = caller_e;
        deliver_now(k, dst);
    } else {
        if non_blocking {
            return ENOTREADY;
        }
        // Check for a possible deadlock before actually blocking.
        if deadlock(k, SEND as i32, caller, dst_e) != 0 {
            return ELOCKED;
        }
        // Destination is not waiting: block the caller and queue it on
        // the destination's caller queue.
        if !copy_from_user(m_ptr, &mut k.proc[caller].p_sendmsg) {
            return crate::minix::EFAULT;
        }
        rts_set(k, caller, RTS_SENDING);
        k.proc[caller].p_sendto_e = dst_e;

        let xpp = k.proc[dst].p_caller_q;
        if xpp.is_none() {
            k.proc[dst].p_caller_q = Some(caller);
        } else {
            // walk to the end of the list
            let mut last = xpp.unwrap();
            while let Some(next) = k.proc[last].p_q_link {
                last = next;
            }
            k.proc[last].p_q_link = Some(caller);
        }
    }
    OK
}

/*===========================================================================*
 *                mini_receive                                               *
 *===========================================================================*/

/// A process wants a message.  If a notification or a message from a
/// sender queued on us is already available, deliver it; otherwise block.
pub unsafe fn mini_receive(
    k: &mut Kernel,
    caller: usize,
    src_e: i32,
    m_buff_usr: *mut Message,
) -> i32 {
    // This is where we want our message.
    k.proc[caller].p_delivermsg_vir = m_buff_usr;

    let src_slot: Option<usize> = if src_e == ANY {
        None
    } else {
        match isokendpt(k, src_e) {
            Some(s) => Some(s),
            None => return crate::minix::EDEADSRCDST,
        }
    };

    // The caller's RTS_SENDING flag may be set if SENDREC couldn't send;
    // if it is set the process stays blocked.
    if !rts_isset(&k.proc[caller], RTS_SENDING) {
        // Pending notifications first, except during SENDREC (the reply
        // must not be overtaken by a notification).
        if k.proc[caller].p_misc_flags & MF_REPLY_PEND == 0 {
            if let Some(found) = take_pending_notify(k, caller, src_slot) {
                let sender_e = k.proc[found].p_endpoint;
                build_notify_message(
                    k.realtime,
                    &mut k.proc[caller].p_delivermsg,
                );
                k.proc[caller].p_delivermsg.m_source = sender_e;
                deliver_now(k, caller);
                return OK;
            }
        }

        // Check the caller queue: senders blocked trying to send to us.
        let mut prev_link: *mut Option<usize> = &mut k.proc[caller].p_caller_q;
        let mut iter = k.proc[caller].p_caller_q;
        while let Some(sender) = iter {
            let sender_e = k.proc[sender].p_endpoint;
            let acceptable = src_e == ANY || src_e == sender_e;
            if acceptable {
                // Found acceptable message: copy it and unblock sender.
                k.proc[caller].p_delivermsg = k.proc[sender].p_sendmsg;
                k.proc[caller].p_delivermsg.m_source = sender_e;
                // Remove sender from queue.
                let next = k.proc[sender].p_q_link;
                *prev_link = next;
                k.proc[sender].p_q_link = None;
                // Sender's send completed.
                rts_unset(k, sender, RTS_SENDING);
                k.proc[sender].p_reg.regs[0] = OK as u64;
                // If this was the SEND of a SENDREC, run its RECEIVE.
                if k.proc[sender].p_misc_flags & MF_REPLY_PEND != 0 {
                    k.proc[sender].p_misc_flags &= !MF_REPLY_PEND;
                    ipc_continuation(k, sender);
                }
                deliver_now(k, caller);
                return OK;
            }
            prev_link = &mut k.proc[sender].p_q_link;
            iter = k.proc[sender].p_q_link;
        }
    }

    // No suitable message available: block the process trying to receive.
    if deadlock(k, RECEIVE as i32, caller, src_e) != 0 {
        return ELOCKED;
    }
    k.proc[caller].p_getfrom_e = src_e;
    rts_set(k, caller, RTS_RECEIVING);
    OK
}

/*===========================================================================*
 *                mini_notify                                                *
 *===========================================================================*/

/// Send an asynchronous, non-blocking notification from `sender_e` to
/// `dst_e`.  If the destination is receiving, deliver a synthesized
/// notification message at once; otherwise mark it pending.
pub unsafe fn mini_notify(k: &mut Kernel, sender_e: i32, dst_e: i32) -> i32 {
    let dst = match isokendpt(k, dst_e) {
        Some(d) => d,
        None => {
            kprintf!("mini_notify: bogus endpoint {}\r\n", dst_e);
            return crate::minix::EDEADSRCDST;
        }
    };

    if will_receive(k, sender_e, dst) && k.proc[dst].p_misc_flags & MF_REPLY_PEND == 0 {
        // the sender must be a live process (endpoint generation check)
        if isokendpt(k, sender_e).is_none() {
            return crate::minix::EDEADSRCDST;
        }
        build_notify_message(k.realtime, &mut k.proc[dst].p_delivermsg);
        k.proc[dst].p_delivermsg.m_source = sender_e;
        deliver_now(k, dst);
        return OK;
    }

    // Destination is not ready: set the pending-notification bit for the
    // sender's table slot (the original uses privilege ids here).
    let bit = sender_slot_bit(sender_e);
    k.proc[dst].p_notify_pending |= bit;
    OK
}

/// First pending notification acceptable to a receiver, if any
/// (`has_pending_notify` + clear).
unsafe fn take_pending_notify(
    k: &mut Kernel,
    caller: usize,
    src_slot: Option<usize>,
) -> Option<usize> {
    let map = k.proc[caller].p_notify_pending;
    if map == 0 {
        return None;
    }
    for slot in 0..k.proc.len() {
        let bit = 1u128 << slot;
        if map & bit == 0 {
            continue;
        }
        if let Some(want) = src_slot {
            if slot != want {
                continue;
            }
        }
        if rts_isset(&k.proc[slot], RTS_SLOT_FREE) {
            continue;
        }
        k.proc[caller].p_notify_pending &= !bit;
        return Some(slot);
    }
    None
}

/// `BuildNotifyMessage()`: the notification payload carries the current
/// uptime in this port (the original packs the pending-signal bitmap).
unsafe fn build_notify_message(uptime: u64, m: &mut Message) {
    m.m_type = NOTIFY_MESSAGE;
    m.m_u.m_u64[0] = uptime;
}

fn sender_slot_bit(sender_e: i32) -> u128 {
    let nr = endpoint_slot(sender_e);
    let idx = (NR_TASKS as i32 + nr) as usize;
    if idx < 128 {
        1u128 << idx
    } else {
        0
    }
}

/*===========================================================================*
 *                deadlock                                                   *
 *===========================================================================*/

/// Check for deadlock: a cycle of blocking sends/receives.  The only
/// non-fatal cycle of two processes is a SEND(REC) met by a RECEIVE.
/// Returns the group size if a deadlock is found, zero otherwise.
pub unsafe fn deadlock(k: &Kernel, function: i32, cp: usize, src_dst_e: i32) -> i32 {
    let mut group_size = 1;
    let mut e = src_dst_e;

    while e != ANY && e != crate::minix::NONE {
        let xp = match isokendpt(k, e) {
            Some(x) => x,
            None => return 0,
        };
        group_size += 1;

        // Last process in the chain has no dependency: cycle cannot close.
        e = blocked_on(&k.proc[xp]);
        if e == crate::minix::NONE {
            return 0;
        }

        if e == k.proc[cp].p_endpoint {
            if group_size == 2 {
                // SEND vs RECEIVE between two processes is fine.
                let flags = k.proc[xp].p_rts_flags;
                if (flags ^ ((function as u32) << 2)) & RTS_SENDING != 0 {
                    return 0;
                }
            }
            kprintf!(
                "deadlock between processes including {}\r\n",
                k.proc[cp].p_name
            );
            return group_size;
        }
    }
    0
}

/*===========================================================================*
 *                do_sync_ipc                                                *
 *===========================================================================*/

/// Ports `do_sync_ipc()` (proc.c): validate the trap and dispatch to
/// mini_send/mini_receive/mini_notify.  `call_nr` arrives in x0 of the
/// saved frame.  Returns the result to store in x0, unless the process
/// blocked (the result is then produced at delivery time).
pub unsafe fn do_ipc(
    k: &mut Kernel,
    caller: usize,
    call_nr: u64,
    src_dst_e: i32,
    m_ptr: *mut Message,
) -> i32 {
    // Remember the call for a possible SENDREC continuation.
    k.proc[caller].p_ipc_srcdst = src_dst_e;
    k.proc[caller].p_ipc_msgptr = m_ptr;

    // Only allow known call numbers.
    if call_nr > SENDREC as u64 && call_nr != crate::minix::NOTIFY as u64 && call_nr != SENDNB as u64
    {
        return EBADCALL;
    }

    // RECEIVE is the only call that accepts ANY.
    if src_dst_e == ANY {
        if call_nr != RECEIVE as u64 {
            return EINVAL;
        }
    } else if isokendpt(k, src_dst_e).is_none() {
        return crate::minix::EDEADSRCDST;
    }

    let mut result;
    match call_nr {
        c if c == SENDREC as u64 => {
            // A flag is set so that notifications cannot interrupt SENDREC.
            k.proc[caller].p_misc_flags |= MF_REPLY_PEND;
            result = mini_send(k, caller, src_dst_e, m_ptr, false);
            if result != OK {
                k.proc[caller].p_misc_flags &= !MF_REPLY_PEND;
                return result;
            }
            // Fall through to the RECEIVE half.
            result = mini_receive(k, caller, src_dst_e, m_ptr);
        }
        c if c == SEND as u64 => {
            result = mini_send(k, caller, src_dst_e, m_ptr, false);
        }
        c if c == RECEIVE as u64 => {
            k.proc[caller].p_misc_flags &= !MF_REPLY_PEND;
            result = mini_receive(k, caller, src_dst_e, m_ptr);
        }
        c if c == crate::minix::NOTIFY as u64 => {
            result = mini_notify(k, k.proc[caller].p_endpoint, src_dst_e);
        }
        c if c == SENDNB as u64 => {
            result = mini_send(k, caller, src_dst_e, m_ptr, true);
        }
        _ => result = EBADCALL,
    }
    result
}

/// Did the caller block inside do_ipc?  The trap layer uses this to
/// decide whether x0 already carries the result.
pub fn blocked_in_ipc(k: &Kernel, p: usize) -> bool {
    rts_isset(&k.proc[p], RTS_SENDING) || rts_isset(&k.proc[p], RTS_RECEIVING)
}
