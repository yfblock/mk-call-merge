#!/usr/bin/env python3
"""
mkimage.py — Build a bootable seL4 image for x86_64 QEMU.

Creates a 32-bit ELF wrapper for the kernel. QEMU's multiboot loader
will load the kernel and pass the root task (via -initrd) as a module.

Usage:
  python3 mkimage.py --kernel kernel.elf -o kernel32.elf
  qemu-system-x86_64 -kernel kernel32.elf -initrd root-task -nographic
"""

import argparse
import struct
import sys
import os


def align_up(val, align):
    return (val + align - 1) & ~(align - 1)


def p16(val):
    return struct.pack("<H", val)


def p32(val):
    return struct.pack("<I", val)


ELFCLASS32 = 1
ELFDATA2LSB = 1
ET_EXEC = 2
EM_386 = 3
EV_CURRENT = 1
PT_LOAD = 1
PF_R = 4
PF_W = 2
PF_X = 1
ELF32_EHDR_SIZE = 52
ELF32_PHDR_SIZE = 32


def parse_elf_segments(data):
    if data[:4] != b"\x7fELF":
        raise ValueError("Not a valid ELF file")
    is64 = data[4] == 2
    endian = "<"
    if is64:
        e_phoff = struct.unpack_from(endian + "Q", data, 32)[0]
        e_phentsize = struct.unpack_from(endian + "H", data, 54)[0]
        e_phnum = struct.unpack_from(endian + "H", data, 56)[0]
        e_entry = struct.unpack_from(endian + "Q", data, 24)[0]
    else:
        e_phoff = struct.unpack_from(endian + "I", data, 28)[0]
        e_phentsize = struct.unpack_from(endian + "H", data, 42)[0]
        e_phnum = struct.unpack_from(endian + "H", data, 44)[0]
        e_entry = struct.unpack_from(endian + "I", data, 24)[0]

    segs = []
    for i in range(e_phnum):
        off = e_phoff + i * e_phentsize
        p_type = struct.unpack_from(endian + "I", data, off)[0]
        if p_type != PT_LOAD:
            continue
        if is64:
            p_offset = struct.unpack_from(endian + "Q", data, off + 8)[0]
            p_paddr = struct.unpack_from(endian + "Q", data, off + 24)[0]
            p_filesz = struct.unpack_from(endian + "Q", data, off + 32)[0]
            p_memsz = struct.unpack_from(endian + "Q", data, off + 40)[0]
        else:
            p_offset = struct.unpack_from(endian + "I", data, off + 4)[0]
            p_paddr = struct.unpack_from(endian + "I", data, off + 12)[0]
            p_filesz = struct.unpack_from(endian + "I", data, off + 16)[0]
            p_memsz = struct.unpack_from(endian + "I", data, off + 20)[0]
        segs.append((p_offset, p_paddr, p_filesz, p_memsz))
    return segs, e_entry, is64


def build_flat_binary(segs, data):
    if not segs:
        return b"", 0, 0
    min_paddr = min(s[1] for s in segs)
    max_paddr = max(s[1] + max(s[2], s[3]) for s in segs)
    size = align_up(max_paddr - min_paddr, 4096)

    buf = bytearray(size)
    for offset, paddr, filesz, memsz in sorted(segs, key=lambda s: s[1]):
        dest = paddr - min_paddr
        buf[dest : dest + filesz] = data[offset : offset + filesz]

    return bytes(buf).ljust(size, b"\x00"), size, min_paddr


def build_kernel_wrapper(kernel_path, output_path):
    with open(kernel_path, "rb") as f:
        kdata = f.read()

    k_segs, k_entry, k_is64 = parse_elf_segments(kdata)
    print(f"Kernel: {len(k_segs)} LOAD segs, entry=0x{k_entry:x}")

    for i, (offset, paddr, filesz, memsz) in enumerate(k_segs):
        print(f"  seg {i}: paddr=0x{paddr:x} filesz=0x{filesz:x}")

    # Build flat binary
    k_bin, k_size, k_min = build_flat_binary(k_segs, kdata)
    k_max = k_min + k_size
    print(f"Kernel flat: 0x{k_min:x}-0x{k_max:x} ({k_size} bytes)")

    # Convert virtual entry to physical if needed
    phys_entry = k_entry
    if k_entry >= 0xFFFFFFFF00000000:
        phys_entry = k_entry - 0xFFFFFFFF80000000 + 0x100000
    print(f"Physical entry: 0x{phys_entry:x}")

    # Check if multiboot header exists in kernel
    mb_found = False
    for i in range(0, min(32768, len(k_bin)), 4):
        val = struct.unpack_from("<I", k_bin, i)[0]
        if val == 0x1BADB002:
            print(f"Multiboot header found at offset 0x{i:x} in flat binary")
            mb_found = True
            break

    if not mb_found:
        print("WARNING: No multiboot header found in kernel!")

    # Create 32-bit ELF wrapper
    kernel_file_off = 0x1000

    # Build ELF32 header
    elf = bytearray()
    elf.extend(b"\x7fELF")
    elf.append(ELFCLASS32)
    elf.append(ELFDATA2LSB)
    elf.append(EV_CURRENT)
    elf.append(0x00)
    elf.extend(b"\x00" * 8)
    elf.extend(p16(ET_EXEC))
    elf.extend(p16(EM_386))
    elf.extend(p32(EV_CURRENT))
    elf.extend(p32(phys_entry))
    elf.extend(p32(ELF32_EHDR_SIZE))
    elf.extend(p32(0))
    elf.extend(p32(0))
    elf.extend(p16(ELF32_EHDR_SIZE))
    elf.extend(p16(ELF32_PHDR_SIZE))
    elf.extend(p16(1))
    elf.extend(p16(0))
    elf.extend(p16(0))
    elf.extend(p16(0))

    # Program header: load kernel to its physical address
    phdr = bytearray()
    phdr.extend(p32(PT_LOAD))
    phdr.extend(p32(kernel_file_off))
    phdr.extend(p32(k_min))
    phdr.extend(p32(k_min))
    phdr.extend(p32(k_size))
    phdr.extend(p32(k_size))
    phdr.extend(p32(PF_R | PF_W | PF_X))
    phdr.extend(p32(4096))

    # Assemble output
    output = bytearray()
    output.extend(elf)
    output.extend(phdr)

    # Pad to kernel offset
    while len(output) < kernel_file_off:
        output.append(0)

    # Add kernel flat binary
    output.extend(k_bin)

    with open(output_path, "wb") as f:
        f.write(output)

    print(f"\nOutput: {output_path} ({len(output)} bytes)")
    print(f"  Entry: 0x{phys_entry:x}")
    print(f"  Kernel: 0x{k_min:x} ({k_size} bytes)")
    print(f"\nRun with:")
    print(f"  qemu-system-x86_64 -kernel {output_path} -initrd <root-task> -nographic")


def main():
    parser = argparse.ArgumentParser(description="Build seL4 kernel wrapper for QEMU")
    parser.add_argument("--kernel", required=True, help="seL4 kernel ELF")
    parser.add_argument("-o", required=True, help="Output file")
    args = parser.parse_args()

    if not os.path.exists(args.kernel):
        print(f"ERROR: not found: {args.kernel}")
        sys.exit(1)

    build_kernel_wrapper(args.kernel, args.o)


if __name__ == "__main__":
    main()
