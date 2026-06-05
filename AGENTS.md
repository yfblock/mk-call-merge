# AGENTS.md

## Build Commands

```bash
# Full build (clones seL4, builds kernel, root-task, image)
make all

# Build components separately
make kernel              # Build seL4 kernel (requires cmake, ninja, gcc)
make build               # Build all Rust components
make image               # Create bootable image (requires kernel first)

# Run in QEMU
make run                 # Boot in QEMU (software emulation)
make run-kvm             # Boot with KVM acceleration (recommended)

# ISO/UEFI targets
make iso                 # Build BIOS-bootable ISO
make iso-run             # Boot ISO in QEMU
make uefi                # Build UEFI-bootable ISO
make uefi-run            # Boot UEFI ISO with OVMF

# Alpine rootfs
make alpine-ext4         # Download Alpine minirootfs and create ext4 image

# Clean
make clean
```

## Required Dependencies

- **Kernel build**: cmake, ninja-build, gcc
- **ISO creation**: grub-mkrescue (auto-downloads grub-pc-bin if missing)
- **Cross-compiled tasks**: musl cross-compiler at `/tmp/x86_64-linux-musl-cross/bin`
- **Rust**: nightly-2025-02-01 (see rust-toolchain.toml)

## Architecture

- **Workspace root**: `Cargo.toml` (Rust 2024 edition)
- **Custom target**: `support/targets/x86_64-sel4.json` (x86_64 only, no_std)
- **Entry point**: `root-task/src/main.rs` with `entry.S` assembly
- **No FFI**: All seL4 interactions use pure Rust inline assembly

## Key Packages

| Package | Description |
|---------|-------------|
| `root-task` | Main entry point, initializes system, runs tests |
| `blk-task` | Block device (ramdisk) implementation |
| `lwext4-task` | ext4 filesystem via lwext4_rust |
| `ext4-srv` | ext4 filesystem service (IPC-based) |
| `lcl` | Linux Compatible Layer - syscall emulation |
| `crates/sel4-sys` | Low-level seL4 syscall wrappers |
| `crates/sel4-ulib` | seL4 userspace library utilities |
| `crates/common` | Shared constants, allocators, slot management |
| `crates/srv-gate` | Service gate abstractions (block, fs, uart) |
| `crates/libc-core` | Minimal libc implementation |

## Testing

Tests run automatically on `make run`. The root task executes:
1. sel4-sys unit tests
2. Block device read/write tests
3. ext4 filesystem tests
4. LCL tests (20 test cases covering syscalls, memory, signals, pipes)
5. Busybox execution test
6. IPC benchmarks

## Common Issues

- **seL4 repo not cloned**: `make kernel` auto-clones from GitHub
- **grub-pc-bin missing**: Auto-downloaded during `make iso`
- **KVM not available**: Use `make run` instead of `make run-kvm`
- **Cross-compiler missing**: Install musl cross-compiler to `/tmp/x86_64-linux-musl-cross/`
- **Alpine ext4 image not found**: Run `make alpine-ext4` to create the image

## Current Status (as of 2025-06-04)

- ✅ System boot and all tests pass (51 tests)
- ✅ Alpine ext4 image creation script
- ✅ ext4-srv service task (IPC-based)
- ✅ LCL IPC client for filesystem operations
- ✅ execve syscall implementation
- ✅ busybox ELF loading works
- ⚠️  busybox execution encounters capability fault
- ⚠️  syscall handling needs improvement
- ⚠️  fork/clone not yet implemented
