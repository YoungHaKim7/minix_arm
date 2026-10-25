//! Exception dispatch: the aarch64 counterpart of the architecture trap
//! entry in `minix/minix/kernel/arch/*/` (`exception()` / `trap()`),
//! feeding into `do_ipc()` and the kernel-call dispatcher like the
//! original's `sys_call()` path.
//!
//! `trap_dispatch` is called from asm/boot.S with a frame saved at the
//! top of the interrupted process's kernel stack; it returns the frame
//! of the process to resume, which may be a different process.

use crate::arch::{read_sysreg, StackFrame};
use crate::kernel::ipc::{blocked_in_ipc, do_ipc};
use crate::kernel::sched::schedule_frame;
use crate::kernel::system::do_kernel_call;
use crate::kernel::{kernel, Kernel};
use crate::kprintf;
use crate::minix::{Message, ESRCH};

/// EC field of ESR_EL1 for "SVC instruction execution in AArch64 state".
const EC_SVC_AARCH64: u64 = 0x15;

/// `svc #0`: IPC trap; `svc #1`: kernel call.
const SVC_IPC: u64 = 0;
const SVC_KERNEL_CALL: u64 = 1;

#[unsafe(no_mangle)]
pub extern "C" fn trap_dispatch(frame: *mut StackFrame, kind: u64) -> *mut StackFrame {
    unsafe {
        let k = kernel();

        // Save the interrupted context into its slot in the process
        // table (p_reg), unless we trapped before the first schedule.
        if let Some(cur) = (*k).proc_ptr {
            core::ptr::copy_nonoverlapping(frame, &mut (*k).proc[cur].p_reg as *mut _, 1);
        }

        match kind {
            0 => sync_from_user(k),
            1 => fault_at_el1(k),
            2 => crate::kernel::clock::clock_interrupt(&mut *k),
            _ => {}
        }

        // Pick a process and hand its context back to the asm resume
        // path (ports restart()).
        schedule_frame(&mut *k)
    }
}

/// Unexpected exception (FIQ/SError/unused vector slot): report and halt.
#[unsafe(no_mangle)]
pub extern "C" fn trap_unexpected(frame: *mut StackFrame, _kind: u64) {
    unsafe {
        let esr = read_sysreg("esr_el1");
        kprintf!(
            "kernel: unexpected exception, esr={:#x} elr={:#x}\r\n",
            esr,
            (*frame).pc
        );
    }
    loop {
        core::hint::spin_loop();
    }
}

/// Synchronous exception from EL0: an IPC/kernel-call trap, or a user
/// fault that kills the process (the original sends a signal through PM).
unsafe fn sync_from_user(k: *mut Kernel) {
    let cur = match (*k).proc_ptr {
        Some(c) => c,
        None => return,
    };
    let esr = read_sysreg("esr_el1");
    let ec = esr >> 26;
    if ec != EC_SVC_AARCH64 {
        let far = read_sysreg("far_el1");
        kprintf!(
            "kernel: {} faulted (ec={:#x} far={:#x}), killing\r\n",
            (*k).proc[cur].p_name, ec, far
        );
        let ep = (*k).proc[cur].p_endpoint;
        crate::kernel::system::sys_exit(&mut *k, ep);
        return;
    }

    let iss = esr & 0xFFFF;
    match iss {
        SVC_IPC => {
            // do_ipc(r1, r2, r3): function in x0, src/dst endpoint in x1,
            // message pointer in x2 of the saved frame.
            let reg = &(*k).proc[cur].p_reg;
            let call_nr = reg.regs[0];
            let srcdst = reg.regs[1] as i32;
            let mptr = reg.regs[2] as *mut Message;
            let r = do_ipc(&mut *k, cur, call_nr, srcdst, mptr);
            if !blocked_in_ipc(&*k, cur) {
                (*k).proc[cur].p_reg.regs[0] = r as u32 as u64;
            }
        }
        SVC_KERNEL_CALL => {
            let mptr = (*k).proc[cur].p_reg.regs[1] as *mut Message;
            if (mptr as usize) & 0x7 != 0 {
                (*k).proc[cur].p_reg.regs[0] = crate::minix::EFAULT as u32 as u64;
                return;
            }
            let r = do_kernel_call(&mut *k, cur, mptr);
            (*k).proc[cur].p_reg.regs[0] = r as u32 as u64;
        }
        _ => {
            (*k).proc[cur].p_reg.regs[0] = ESRCH as u32 as u64;
        }
    }
}

/// Synchronous exception at EL1: a kernel bug.  Report and halt.
unsafe fn fault_at_el1(k: *mut Kernel) {
    let esr = read_sysreg("esr_el1");
    let far = read_sysreg("far_el1");
    let cur = (*k).proc_ptr.map(|c| (*k).proc[c].p_name).unwrap_or("?");
    let pc = (*k)
        .proc_ptr
        .map(|c| (*k).proc[c].p_reg.pc)
        .unwrap_or(0);
    kprintf!(
        "kernel: EXCEPTION at EL1 in {}, pc={:#x} esr={:#x} far={:#x}\r\n",
        cur, pc, esr, far
    );
    kprintf!("kernel: halting\r\n");
    loop {
        core::hint::spin_loop();
    }
}
