# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**rel4-linux-kit** runs the seL4 microkernel on x86_64 with a Linux-compatible syscall emulation layer (LCL). The goal is to run Linux ELF binaries (like busybox) on seL4 by intercepting syscalls and translating them into seL4 IPC operations.

**Long-term goal**: run `busybox iozone` against an ext4 filesystem — i.e. the emulated Linux process performs real file I/O (open/read/write/lseek/stat on ext4 files) through the LCL, backed by the ext4-srv → lwext4-task → blk-task stack. This requires the LCL's file syscalls (open/openat, read, write, lseek, stat/fstat, close, getdents64, etc.) to be wired through to the ext4 IPC service instead of the current stubs.

## Build Commands

```bash
make all              # Full build: clone seL4 + build kernel + build root-task + create image
make build            # Build all Rust binaries (root-task, blk-task, lwext4-task, ext4-srv)
make kernel           # Build seL4 kernel (auto-clones seL4 repo if missing)
make image            # Create bootable ELF image (requires kernel)
make run              # Boot in QEMU (software emulation)
make run-kvm          # Boot in QEMU with KVM acceleration
make iso              # Build BIOS-bootable GRUB ISO
make uefi             # Build UEFI-bootable ISO
make alpine-ext4      # Download Alpine minirootfs and create 32MB ext4 image
make clean            # cargo clean + remove image.elf
```

There is no separate `make test` — tests run automatically during `make run` via QEMU serial output.

## Build System Details

The project uses a **Makefile + Cargo workspace** hybrid. The Makefile orchestrates the full build pipeline; Cargo handles Rust compilation.

Cargo flags (set by Makefile):
```
--target x86_64-sel4 --release -Z build-std=core,alloc,compiler_builtins -Z build-std-features=compiler-builtins-mem
```

**Custom target**: `support/targets/x86_64-sel4.json` — `#![no_std]`, static relocation, no redzone, soft-float (no SSE/MMX), linked with `rust-lld`.

**Cross-compilation**: `blk-task` and `lwext4-task` are cross-compiled with musl at `/tmp/x86_64-linux-musl-cross/bin`. `root-task` and `ext4-srv` compile for the custom seL4 target.

**Toolchain**: nightly-2025-02-01 (pinned in `rust-toolchain.toml`).

## Architecture

```
root-task (main)
  ├── lcl (Linux Compatible Layer — syscall emulation)
  │     ├── syscall/     — exec, fs, mm, signal, sys, thread handlers
  │     ├── task/        — PCB, runner, memory, file, signal management
  │     ├── fs/          — DevFS (null/zero/stdin/stdout), pipe, IPC client for ext4-srv
  │     └── arch/x86_64/ — context switching
  │
  ├── ext4-srv (IPC service — file operations via seL4 Endpoint)
  │     └── delegates to lwext4-task for actual ext4 I/O
  │
  ├── blk-task (block device — ramdisk holding the ext4 image)
  └── lwext4-task (ext4 filesystem operations via lwext4_rust)
```

Communication between tasks uses **seL4 Endpoint IPC**. The LCL's `fs/ipc_client.rs` is the client side; `ext4-srv/src/service.rs` is the server side.

### Key Design Decisions

1. **No FFI** — all seL4 kernel interactions use pure Rust inline assembly (`core::arch::asm!`)
2. **x86_64 only** — custom target spec, no cross-platform support
3. **`#![no_std]`** — uses `core` and `alloc` only
4. **seL4 syscall convention** (x86_64): syscall number in `rdx`, message registers `rdi/rsi/r10/r8/r9/r12/r13/r15`, `rsp` saved/restored via `r14` (kernel clobbers it)

### Library Crates (`crates/`)

| Crate | Role |
|-------|------|
| `sel4-sys` | Low-level seL4 syscall wrappers (pure asm), boot info, IPC buffer, types, tests |
| `sel4-ulib` | seL4 userspace utilities |
| `common` | Shared constants, allocators, slot management |
| `srv-gate` | Service gate abstractions (block, fs, uart) |
| `libc-core` | Minimal libc (errno, fcntl, types) |

## Testing

Tests run inside QEMU as part of `make run`. The root task (`root-task/src/main.rs`) executes:

1. **sel4-sys unit tests** — 48+ tests in `crates/sel4-sys/src/tests.rs` (MessageInfo, CapRights, UserContext, IPC buffer, etc.)
2. **Block device tests** — ramdisk read/write verification
3. **ext4 filesystem tests** — mkdir, open, write, read, stat, close
4. **LCL tests** — 20 cases covering syscall dispatch, memory layout, ELF parsing, DevFS, pipes, signals, PCB, timers
5. **Busybox execution test** — loads ELF, creates user task, handles faults
6. **IPC benchmarks**

Test output goes to seL4 debug serial. On completion, the system triggers QEMU's `isa-debug-exit` device to shut down.

## Build Dependencies

- **Kernel**: cmake, ninja-build, gcc
- **ISO**: grub-mkrescue (grub-pc-bin auto-downloaded if missing)
- **UEFI**: OVMF firmware (`/usr/share/ovmf/OVMF.fd`)
- **Cross-compiled tasks**: musl cross-compiler at `/tmp/x86_64-linux-musl-cross/bin`
- **Alpine ext4**: needs internet access for `make alpine-ext4`

## Common Issues

- **seL4 repo not cloned**: `make kernel` auto-clones from GitHub
- **Cross-compiler missing**: install musl cross-compiler to `/tmp/x86_64-linux-musl-cross/`
- **KVM not available**: use `make run` (software emulation) instead of `make run-kvm`
- **Alpine ext4 image not found**: run `make alpine-ext4` first
- **QEMU exits immediately**: check if `isa-debug-exit` is configured; test output is on serial console

## Current Status

busybox runs on seL4: it executes to completion, runs shell scripts (`sh -c "..."`), and supports an interactive shell with a `/ # ` prompt (driven via COM1 serial input). The child runs in the root task's VSpace; syscalls are emulated by handling UnknownSyscall faults in `root-task/src/main.rs` (`test_busybox()`), with ELF loading and task setup in `lcl/src/task/runner.rs`.

### Toward the long-term goal (busybox iozone on ext4)

The remaining work is making the LCL's **file syscalls operate on a real ext4 filesystem** rather than returning stubs:

- File syscalls are currently stubbed: `open`/`openat` → ENOENT, `read(fd>2)` → EOF, `write`/`writev` → console only, `stat`/`fstat`/`getdents64`/`lseek` → mostly unimplemented.
- Wire these through the existing IPC path: LCL `fs/ipc_client.rs` → `ext4-srv/src/service.rs` → `lwext4-task` → `blk-task` ramdisk (holds the ext4 image).
- Maintain a per-process fd table in the LCL mapping fds to ext4 inodes/handles.
- `iozone` exercises sequential/random read/write, lseek, file create/delete, and fstat — each needs a working path to the ext4 service.

Note: the bundled busybox (`http-boot/busybox`, gitignored) is a static glibc build. A static musl busybox was tried but crashes during libc init under the current loader; the glibc one is the working binary.

