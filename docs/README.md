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
