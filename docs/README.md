# wiki.minix3

- https://wiki.minix3.org/doku.php?id=developersguide:usinggit

```bash
git clone http://repo.or.cz/r/minix.git
```
# I'll implement the Rust port — a MINIX-style multiserver microkernel for aarch64/QEMU

- Study complete. Now I'll implement the Rust port — a MINIX-style multiserver microkernel for aarch64/QEMU, mirroring the original module structure. Updating todos and writing the code:

- Design note before I start: converting the entire MINIX OS (kernel + full NetBSD userland) to Rust isn't a single-session task, so I'm doing what the project skeleton clearly intends: a faithful Rust port of the MINIX microkernel core, running on the aarch64 QEMU target that's already configured. Each Rust module corresponds 1:1 to its original C file:

| Rust module | Converts from minix/ |
| --- | --- |
| src/minix/{endpoint,ipc,callnr,com}.rs | include/minix/{endpoint,ipc,callnr,com}.h |
| src/kernel/{proc,sched,ipc,clock,system}.rs | kernel/{proc.h,proc.c,clock.c,system.c} |
| src/arch.rs + asm/boot.S | kernel/arch/* (aarch64 replaces i386/earm) |
| src/servers/{pm,tty,init}.rs | servers/pm, drivers/tty, servers/init |

# 만들어야할 부분 

```bash
Study MINIX kernel sources (proc, ipc, main, scheduler)

Write minix types: message, endpoints, call numbers
Write aarch64 arch layer: boot.S, vectors, MMU, GIC, timer
Write kernel core: proc table, RTS flags, scheduler
Write MINIX IPC: do_ipc, mini_send/receive/notify, deadlock
Write clock task + kernel calls (system)
Write user-mode servers: PM, TTY, init demo
Build for aarch64 and boot-test in QEMU
```

| Original C (minix/minix/) | Rust port |
| --- | --- |
| include/minix/{ipc,endpoint,callnr,com}.h | src/minix/ — the 64-byte message type with m_source/m_type and the mess_* payload unions, generation-aware endpoints, PM/VFS/KERNEL_CALL/CDEV numbering, MINIX errno values |
| kernel/proc.h + proc.c scheduler | src/kernel/proc.rs, sched.rs — the process table, full RTS flag semantics, 16 run queues, enqueue/dequeue/pick_proc, quantum expiry, preemption |
| kernel/proc.c IPC | src/kernel/ipc.rs — do_sync_ipc, mini_send, mini_receive, mini_notify with pending-notification bitmaps, SENDREC continuation, the deadlock() cycle detector |
| kernel/clock.c + system.c | clock.rs (ticks, alarms), system.rs (kernel-call dispatch: sys_setalarm, sys_times, sys_exit with endpoint-generation bump) |
| kernel/arch/* | asm/boot.S + src/arch.rs (EL2→EL1 boot, exception vectors, MMU, GICv2, generic timer) + kernel/trap.rs |
| servers/pm, drivers/tty, servers/init | src/servers/ — running as real EL0 user processes that enter the kernel via svc traps |

- The boot demo exercises the real MINIX mechanisms end-to-end: `init` prints through the TTY server via `CDEV_WRITE` sendrecs, gets its pid from the PM server, sets a 2-second alarm through the POSIX-style chain (`init` → `PM_ITIMER` → `sys_setalarm` → `CLOCK tick` → `notify PM` → `PM forwards the notification` — it wakes at exactly uptime 200 ticks), and exits via `PM_EXIT` → `sys_exit`.

- Run it with `just qemu`.

- Debugging war stories (the interesting part)

- Four platform quirks each produced silent failure and are worth knowing:
  - 1. EL1 may not execute from pages writable at EL0. QEMU enforces this (I confirmed it in QEMU's `ptw.c`: get_S1prot). A flat RW map therefore permission-faults every kernel fetch. The fix is the three-zone design now in `asm/linker.ld`: kernel text RO+executable at both ELs (0x40080000), kernel data EL1-only (0x40200000), user memory RW-never-executed (0x40400000).
  - 2. Exception vector slots are 128 bytes. My packed 4-byte entries made exceptions land mid-instruction inside adjacent handlers, producing an exception storm that masqueraded as a GIC problem.
  - 3. With `T0SZ=25` the page walk starts at level 1, so TTBR0 points directly at the block table — my extra L0 table caused translation faults.
  - 4. Rust emits NEON stores for 64-byte message copies, so FP/SIMD must be enabled (`CPACR_EL1`) before entering EL0.
- Not converted (noted in the README): VFS, VM, the sched/RS/DS servers, signals, SENDA, safe-copies, SMP, and fork/exec. The build is warning-free, and I saved the platform quirks and port scope to memory for future sessions.
