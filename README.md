# minix_arm — the MINIX 3 kernel converted to Rust for aarch64

A Rust port of the core of the MINIX 3 multiserver microkernel
(`minix/` — the upstream git tree is vendored in this repo), retargeted
from i386 to aarch64, running on the QEMU `virt` machine.

Reference: https://wiki.minix3.org/

## Result

```
$ just qemu            # or: cargo build && qemu-system-aarch64 -machine virt -cpu cortex-a57 -m 256M -nographic -kernel target/aarch64-unknown-none/debug/minix_arm

MINIX 3.4.0 (Rust port to aarch64/QEMU-virt)
Multiserver microkernel: -4, -3, -2 tasks ready
Kernel: proc/sched/ipc/clock/system converted from C
Servers at EL0: PM, TTY; userland: init


== userland: init started ==
Hello from userland init via MINIX IPC!
init: PM says my pid is 2
init: sleeping 2s on a MINIX alarm...
init: wakeup notification from PM at uptime 200 ticks
init: exiting via PM_EXIT -> sys_exit
kernel: init terminated by sys_exit (pid 2)
```

What the boot demonstrates, all through the original MINIX mechanisms:

- `init` (an unprivileged EL0 process) talks to the servers with
  `send`/`sendrec`/`receive` IPC traps (64-byte `message`, `m_source`,
  `m_type`), exactly as in `do_sync_ipc()`.
- Console output goes through the TTY character-device server
  (`CDEV_WRITE`), not the kernel.
- `PM_GETPID` and `PM_EXIT` are served by the Process Manager at EL0.
- The POSIX alarm chain: `init` -> `PM_ITIMER` (PM) -> `sys_setalarm`
  (kernel call, SYSTEM role) -> CLOCK tick -> notification to PM ->
  notification forwarded to the sleeping process.
- `PM_EXIT` ends in the kernel's `sys_exit()`, which frees the slot and
  bumps the endpoint generation (`EDEADSRCDST` semantics).
- Preemption, 16-queue round-robin scheduling with time quanta, the
  IDLE task, and RTS flags (`RTS_SENDING`, `RTS_RECEIVING`,
  `RTS_PREEMPTED`, `RTS_NO_QUANTUM`, ...) behave like the originals.

## What is converted (C source -> Rust module)

| Original (`minix/minix/`)                    | Port                          |
| -------------------------------------------- | ----------------------------- |
| `include/minix/ipc.h`, `ipcconst.h`          | `src/minix/ipc.rs`            |
| `include/minix/endpoint.h`                   | `src/minix/endpoint.rs`       |
| `include/minix/callnr.h`                     | `src/minix/callnr.rs`         |
| `include/minix/com.h` (+ errno)              | `src/minix/com.rs`            |
| `include/minix/config.h`                     | `src/config.rs`               |
| `kernel/main.c` (`kmain`/`announce`)         | `src/main.rs`                 |
| `kernel/proc.h` (`struct proc`, RTS flags)   | `src/kernel/proc.rs`          |
| `kernel/proc.c` (scheduler: enqueue/…)       | `src/kernel/sched.rs`         |
| `kernel/proc.c` (IPC: mini_send/…)           | `src/kernel/ipc.rs`           |
| `kernel/proc.c` (`deadlock`)                 | `src/kernel/ipc.rs`           |
| `kernel/clock.c` + `watchdog.c`              | `src/kernel/clock.rs`         |
| `kernel/system.c` (kernel-call dispatch)     | `src/kernel/system.rs`        |
| `kernel/arch/*` (trap entry, context switch) | `src/kernel/trap.rs`, `asm/boot.S`, `src/arch.rs` |
| `servers/pm` (slice of its main loop)        | `src/servers/pm.rs`           |
| `drivers/tty` (CDEV write path)              | `src/servers/tty.rs`          |
| `servers/init` + hello test                  | `src/servers/init.rs`         |

## Deliberate simplifications

- **aarch64/QEMU virt instead of i386**: `svc` traps replace
  sysenter/syscall; the GICv2 interrupt controller, PL011 UART and the
  generic timer replace the PC hardware; exception vectors replace
  `kernel/arch/i386/mpx.S`.
- **One flat address space** instead of VM-managed per-process page
  tables: a single stage-1 identity map with three 2MB permission zones
  (kernel text RO+X at EL0/EL1, kernel data EL1-only, user memory
  RW without execute).  This is why `asm/linker.ld` splits the image
  into 2MB-aligned zones: EL1 may not execute from pages writable at
  EL0.
- **Blocking kernel tasks** are not ported; the clock interrupt is
  serviced in the kernel and the CLOCK task slot stays parked in
  RECEIVE, like the original waiting for `HARD_INT`.
- **Not converted** (yet): VFS, VM, RS, DS, the scheduler server,
  signals (`sys_sigsend` et al.), `SENDA` batched async sends, safe
  copies/copy grants, SMP, forks/execs (no ELF loader for userland).

## Build & run

```bash
cargo build                                   # target comes from .cargo/config.toml
qemu-system-aarch64 -machine virt -cpu cortex-a57 -m 256M -nographic \
    -kernel target/aarch64-unknown-none/debug/minix_arm
# quit QEMU with Ctrl-A x
```

`just qemu` wraps the same command.
