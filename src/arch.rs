//! aarch64 machine layer for the QEMU "virt" machine.  This port replaces
//! `minix/minix/kernel/arch/` (which has i386 and earm): exception frame,
//! MMU setup, interrupt controller (GICv2) and the generic timer.

use crate::config::SYSTEM_HZ;
use core::arch::asm;

/*===========================================================================*
 *                Saved process context (stackframe_s)                       *
 *===========================================================================*/

/// Saved register state of a process, the aarch64 `stackframe_s`.  The
/// layout must match the frame built by SAVE_ALL in asm/boot.S.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct StackFrame {
    pub regs: [u64; 31], // x0 .. x30
    pub sp: u64, // sp_el0
    pub pc: u64, // elr_el1
    pub pstate: u64, // spsr_el1
}

const _: () = assert!(core::mem::size_of::<StackFrame>() == 272);

impl StackFrame {
    pub const ZERO: StackFrame = StackFrame {
        regs: [0; 31],
        sp: 0,
        pc: 0,
        pstate: 0,
    };
}

// PSTATE bits for spsr values.
const SPSR_D: u64 = 1 << 9;
const SPSR_A: u64 = 1 << 6;
const SPSR_MODE_EL0T: u64 = 0b0000;
const SPSR_MODE_EL1H: u64 = 0b0101;

/// User processes run at EL0 with the timer interrupt enabled so the
/// scheduler can preempt them (ports the preemptible user quantum).
pub const SPSR_USER: u64 = SPSR_MODE_EL0T | SPSR_D | SPSR_A;

/// Kernel threads (IDLE) run at EL1h, also interruptible.
pub const SPSR_KERNEL: u64 = SPSR_MODE_EL1H | SPSR_D | SPSR_A;

unsafe extern "C" {
    pub static vector_table: u8;
    fn idle_entry();
    fn eret_frame(frame: *mut StackFrame) -> !;
}

/*===========================================================================*
 *                MMU: identity map so EL0 processes can run                 *
 *===========================================================================*
 * MINIX gives every process its own address space through VM; this port
 * keeps a single flat identity map (one normal-memory region for RAM, one
 * device region for the peripherals), which is enough to run the EL0
 * servers on QEMU virt.
 */

#[repr(C, align(4096))]
struct PageTable {
    entries: [u64; 512],
}

static mut L0_TABLE: PageTable = PageTable { entries: [0; 512] };
static mut L1_TABLE: PageTable = PageTable { entries: [0; 512] };

// Stage-1 block descriptor bits.
const DESC_BLOCK: u64 = 0b01;
const ATTR_MAIR_IDX_SHIFT: u64 = 2;
const ATTR_NORMAL: u64 = 0 << ATTR_MAIR_IDX_SHIFT; // MAIR attr 0: WB cacheable
const ATTR_DEVICE: u64 = 1 << ATTR_MAIR_IDX_SHIFT; // MAIR attr 1: Device nGnRE
const AP_EL0_EL1: u64 = 1 << 6; // allow access from EL0 as well
const SH_INNER_SHAREABLE: u64 = 3 << 8;
const ACCESS_FLAG: u64 = 1 << 10;
const PXN: u64 = 1 << 53; // privileged execute never
const UXN: u64 = 1 << 54; // unprivileged execute never

pub unsafe fn mmu_init() {
    // 48-bit VA space (T0SZ=16): L0 table -> L1 1GB blocks.  (A start
    // level of 1 with 39-bit VAs also maps the same memory, but QEMU's
    // cortex-a57 faults EL1 fetches on level-1 blocks with AP[1] set.)
    L0_TABLE.entries[0] = (&raw const L1_TABLE) as u64 | 0b11; // table descriptor

    // 0x0000_0000 .. 0x3FFF_FFFF: peripherals (GIC at 0x0800_0000, PL011
    // UART at 0x0900_0000).  Device memory, never executable.
    L1_TABLE.entries[0] = 0x0000_0000
        | ATTR_DEVICE
        | AP_EL0_EL1
        | SH_INNER_SHAREABLE
        | ACCESS_FLAG
        | UXN
        | PXN
        | DESC_BLOCK;

    // 0x4000_0000 .. 0x7FFF_FFFF: RAM (kernel image + process memory),
    // normal cacheable, executable at EL0.
    L1_TABLE.entries[1] = 0x4000_0000
        | ATTR_NORMAL
        | AP_EL0_EL1
        | SH_INNER_SHAREABLE
        | ACCESS_FLAG
        | DESC_BLOCK;

    asm!("dsb ish");
    asm!("tlbi vmalle1");
    asm!("dsb ish");
    asm!("isb");

    // MAIR: attr 0 = write-back cacheable, attr 1 = Device nGnRE.
    write_sysreg("mair_el1", 0xFF | (0x04 << 8));
    // TCR: 39-bit VA (T0SZ=25), 4KB granule, inner-shareable WB, 40-bit PA,
    // TTBR1 walks disabled.
    write_sysreg(
        "tcr_el1",
        (2 << 32) | (1 << 23) | (3 << 12) | (1 << 10) | (1 << 8) | 16,
    );
    write_sysreg("ttbr0_el1", (&raw const L0_TABLE) as u64);
    asm!("isb");

    let sctlr = read_sysreg("sctlr_el1");
    // Clear WXN (writable-implies-execute-never): QEMU's reset value has
    // it set, which would make the writable RAM block non-executable.
    let sctlr = sctlr & !(1 << 19);
    write_sysreg("sctlr_el1", sctlr | (1 << 0) | (1 << 2) | (1 << 12)); // M | C | I
    asm!("isb");
}

/*===========================================================================*
 *                Interrupt controller: GICv2 (GIC-400)                      *
 *===========================================================================*/

const GICD_BASE: usize = 0x0800_0000;
const GICC_BASE: usize = 0x0801_0000;

const GICD_CTLR: usize = GICD_BASE + 0x000;
const GICD_ISENABLER: usize = GICD_BASE + 0x100;
const GICD_IPRIORITYR: usize = GICD_BASE + 0x400;
const GICD_ITARGETSR: usize = GICD_BASE + 0x800;

const GICC_CTLR: usize = GICC_BASE + 0x00;
const GICC_PMR: usize = GICC_BASE + 0x04;
const GICC_IAR: usize = GICC_BASE + 0x0C;
const GICC_EOIR: usize = GICC_BASE + 0x10;

/// INTID of the EL1 physical timer (PPI) on the QEMU virt machine.
pub const IRQ_TIMER: u32 = 30;

pub unsafe fn gic_init() {
    mmio_write(GICD_CTLR, 0);
    // Enable the timer interrupt, lowest priority, targeted at CPU 0.
    mmio_write(GICD_ISENABLER + (IRQ_TIMER as usize / 32) * 4, 1 << (IRQ_TIMER % 32));
    mmio_write8(GICD_IPRIORITYR + IRQ_TIMER as usize, 0xA0);
    mmio_write8(GICD_ITARGETSR + IRQ_TIMER as usize, 0x01);
    mmio_write(GICD_CTLR, 1);
    mmio_write(GICC_PMR, 0xFF);
    mmio_write(GICC_CTLR, 1);
}

/// Acknowledge the highest-priority pending interrupt; 1023 = spurious.
pub unsafe fn gic_ack() -> u32 {
    mmio_read(GICC_IAR)
}

pub unsafe fn gic_eoi(iar: u32) {
    mmio_write(GICC_EOIR, iar);
}

/*===========================================================================*
 *                Generic timer (ports kernel/clock.c hardware part)         *
 *===========================================================================*/

static mut TIMER_DELTA: u64 = 0;

pub unsafe fn timer_init() {
    let freq = read_sysreg("cntfrq_el0");
    TIMER_DELTA = freq / SYSTEM_HZ;
    let now = read_sysreg("cntpct_el0");
    write_sysreg("cntp_cval_el0", now + TIMER_DELTA);
    write_sysreg("cntp_ctl_el0", 1); // enable, not masked
}

/// Rearm the timer after a tick; absolute stepping avoids drift.
pub unsafe fn timer_rearm() {
    let cval = read_sysreg("cntp_cval_el0");
    write_sysreg("cntp_cval_el0", cval + TIMER_DELTA);
}

#[allow(dead_code)]
pub fn uptime_ticks() -> u64 {
    unsafe { read_sysreg("cntpct_el0") / if TIMER_DELTA == 0 { 1 } else { TIMER_DELTA } }
}

/*===========================================================================*
 *                Entry points from asm/boot.S                               *
 *===========================================================================*/

pub unsafe fn early_init() {
    // Enable FP/SIMD at EL0 and EL1: Rust's 64-byte Message copies
    // compile to NEON instructions (CPACR_EL1.FPEN).
    write_sysreg("cpacr_el1", 3 << 20);
    asm!("isb");
    write_sysreg("vbar_el1", &vector_table as *const u8 as u64);
    asm!("isb");
    mmu_init();
    gic_init();
}

/// Ports `switch_to_user()` (kernel/proc.c): load the given context and
/// never come back.
pub unsafe fn switch_to_user(frame: *mut StackFrame) -> ! {
    eret_frame(frame)
}

pub fn idle_entry_addr() -> usize {
    (idle_entry as unsafe extern "C" fn()) as usize
}

/*===========================================================================*
 *                Small helpers                                              *
 *===========================================================================*/

pub unsafe fn mmio_read(addr: usize) -> u32 {
    core::ptr::read_volatile(addr as *const u32)
}

pub unsafe fn mmio_write(addr: usize, v: u32) {
    core::ptr::write_volatile(addr as *mut u32, v);
}

pub unsafe fn mmio_write8(addr: usize, v: u8) {
    core::ptr::write_volatile(addr as *mut u8, v);
}

pub unsafe fn read_sysreg(reg: &str) -> u64 {
    let v: u64;
    unsafe {
        match reg {
            "cntfrq_el0" => asm!("mrs x0, cntfrq_el0", lateout("x0") v),
            "cntpct_el0" => asm!("mrs x0, cntpct_el0", lateout("x0") v),
            "cntp_cval_el0" => asm!("mrs x0, cntp_cval_el0", lateout("x0") v),
            "cntp_tval_el0" => asm!("mrs x0, cntp_tval_el0", lateout("x0") v),
            "cntp_ctl_el0" => asm!("mrs x0, cntp_ctl_el0", lateout("x0") v),
            "esr_el1" => asm!("mrs x0, esr_el1", lateout("x0") v),
            "far_el1" => asm!("mrs x0, far_el1", lateout("x0") v),
            "sctlr_el1" => asm!("mrs x0, sctlr_el1", lateout("x0") v),
            "spsr_el1" => asm!("mrs x0, spsr_el1", lateout("x0") v),
            "mair_el1" => asm!("mrs x0, mair_el1", lateout("x0") v),
            "tcr_el1" => asm!("mrs x0, tcr_el1", lateout("x0") v),
            "ttbr0_el1" => asm!("mrs x0, ttbr0_el1", lateout("x0") v),
            "CurrentEL" => asm!("mrs x0, CurrentEL", lateout("x0") v),
            "spsel" => asm!("mrs x0, SPSel", lateout("x0") v),
            _ => panic!("read_sysreg: unknown register"),
        }
    }
    v
}

pub unsafe fn write_sysreg(reg: &str, v: u64) {
    unsafe {
        match reg {
            "mair_el1" => asm!("msr mair_el1, x0", in("x0") v),
            "tcr_el1" => asm!("msr tcr_el1, x0", in("x0") v),
            "ttbr0_el1" => asm!("msr ttbr0_el1, x0", in("x0") v),
            "sctlr_el1" => asm!("msr sctlr_el1, x0", in("x0") v),
            "vbar_el1" => asm!("msr vbar_el1, x0", in("x0") v),
            "cntp_cval_el0" => asm!("msr cntp_cval_el0, x0", in("x0") v),
            "cntp_ctl_el0" => asm!("msr cntp_ctl_el0, x0", in("x0") v),
            "cpacr_el1" => asm!("msr cpacr_el1, x0", in("x0") v),
            _ => panic!("write_sysreg: unknown register"),
        }
    }
}
