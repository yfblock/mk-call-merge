#!/usr/bin/env python3
"""
mkimage.py — Build a bootable seL4 image for x86_64 QEMU.

Creates a PVH-compatible ELF by combining a minimal 32-bit boot trampoline,
the seL4 kernel, and the root task.

Requires: Python 3.6+

Usage:
  python3 mkimage.py --kernel kernel.elf --app root-task -o image.elf
  qemu-system-x86_64 -kernel image.elf -nographic -serial mon:stdio
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


def p64(val):
    return struct.pack("<Q", val)


# ELF32 constants (use 32-bit ELF for multiboot compatibility with QEMU)
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

# XEN PVH ELF note constants
XEN_ELFNOTE_PHYS32_ENTRY = 18


def parse_elf_segments(data):
    """Parse LOAD segments from an ELF64 file. Returns (segs, entry)."""
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
            p_flags = struct.unpack_from(endian + "I", data, off + 4)[0]
        else:
            p_offset = struct.unpack_from(endian + "I", data, off + 4)[0]
            p_paddr = struct.unpack_from(endian + "I", data, off + 12)[0]
            p_filesz = struct.unpack_from(endian + "I", data, off + 16)[0]
            p_memsz = struct.unpack_from(endian + "I", data, off + 20)[0]
            p_flags = struct.unpack_from(endian + "I", data, off + 28)[0]
        segs.append((p_offset, p_paddr, p_filesz, p_memsz, p_flags))
    return segs, e_entry


def build_flat_binary(segs, data, load_base):
    """Build a flat binary image from ELF LOAD segments placed at load_base."""
    if not segs:
        return b"", 0
    min_paddr = min(s[1] for s in segs)
    max_paddr = max(s[1] + max(s[2], s[3]) for s in segs)
    size = align_up(max_paddr - min_paddr, 4096)

    buf = bytearray(size)
    for offset, paddr, filesz, memsz, _flags in sorted(segs, key=lambda s: s[1]):
        dest = paddr - min_paddr
        buf[dest : dest + filesz] = data[offset : offset + filesz]

    return bytes(buf).ljust(size, b"\x00"), size, min_paddr


def build_xen_note(entry32):
    """Build a XEN PVH ELF note for PHYS32_ENTRY."""
    # ELF note format:
    #   namesz: 4 bytes (including null terminator)
    #   descsz: 4 bytes
    #   type:   4 bytes
    #   name:   "Xen\0" (4 bytes, padded to 4-byte alignment)
    #   desc:   entry point (4 bytes, padded to 4-byte alignment)
    desc = struct.pack("<I", entry32)
    namesz = 4    # "Xen\0"
    descsz = 4    # uint32 entry point
    note_type = XEN_ELFNOTE_PHYS32_ENTRY

    data = bytearray()
    data.extend(p32(namesz))
    data.extend(p32(descsz))
    data.extend(p32(note_type))
    data.extend(b"Xen\0")
    data.extend(desc)
    return bytes(data)


def build_multiboot1_header(load_addr, load_end, entry_addr, header_addr):
    """Build a Multiboot 1 header (QEMU 10.0 only supports multiboot1).

    Multiboot 1 header layout (12 bytes minimum, 48 bytes with addresses):
      Offset  Size  Description
      0       4     magic (0x1BADB002)
      4       4     flags (bit 0: page align, bit 1: memory info, bit 16: addresses)
      8       4     checksum: -(magic + flags)
      12      4     header_addr: physical address of the header
      16      4     load_addr: physical load address
      20      4     load_end_addr: exclusive end of load region
      24      4     bss_end_addr: end of BSS
      28      4     entry_addr: entry point

    NOTE: QEMU uses the ELF entry point when loading via ELF. The multiboot
    header provides the physical load address for the kernel. QEMU loads
    the ELF first, then uses the multiboot info to set up the boot.
    """
    flags = 0x00010003  # bit 0 (page align) | bit 1 (memory info) | bit 16 (addresses)
    checksum = (-(0x1BADB002 + flags)) & 0xFFFFFFFF

    hdr = bytearray()
    hdr.extend(p32(0x1BADB002))   # magic
    hdr.extend(p32(flags))         # flags
    hdr.extend(p32(checksum))      # checksum
    hdr.extend(p32(header_addr))   # header_addr
    hdr.extend(p32(load_addr))     # load_addr
    hdr.extend(p32(load_end))      # load_end_addr
    hdr.extend(p32(bss_end))       # bss_end_addr
    hdr.extend(p32(entry_addr))    # entry_addr
    return bytes(hdr)


# Minimal 32-bit boot trampoline (placed after the multiboot2 header)
# Just passes multiboot info to kernel and jumps to 0x100000.
TRAMPOLINE_32 = bytes([
    0xFA,                               # cli
    0xFC,                               # cld
    0xB8, 0x89, 0x62, 0xD7, 0x36,     # mov $0x36D76289, %eax  (multiboot2 magic)
    0xB9, 0x00, 0x00, 0x10, 0x00,     # mov $0x100000, %ecx
    0xFF, 0xE1,                         # jmp *%ecx
    0xF4,                               # hlt
    0xEB, 0xFD,                         # jmp -3
])


def build_image(kernel_path, app_path, output_path):
    with open(kernel_path, "rb") as f:
        kdata = f.read()
    with open(app_path, "rb") as f:
        adata = f.read()

    k_segs, k_entry = parse_elf_segments(kdata)
    a_segs, a_entry = parse_elf_segments(adata)
    print(f"Kernel: {len(k_segs)} LOAD segs, entry=0x{k_entry:x}")
    print(f"Root task: {len(a_segs)} LOAD segs, entry=0x{a_entry:x}")

    # === Layout ===
    # 0x00000000: XEN PVH note + multiboot2 header + 32-bit trampoline
    # 0x00100000: seL4 kernel (1 MiB)
    # 0x02000000: root task (32 MiB)

    TRAMPOLINE_BASE = 0x0
    KERNEL_BASE = 0x100000
    APP_BASE = 0x2000000

    # Build kernel flat binary
    k_bin, k_bin_size, k_min = build_flat_binary(k_segs, kdata, KERNEL_BASE)
    k_size = max(k_bin_size, max(s[1] + max(s[2], s[3]) - KERNEL_BASE for s in k_segs))
    k_size = align_up(int(k_size), 4096)
    k_end = KERNEL_BASE + k_size

    # Build app flat binary
    a_min = min(s[1] for s in a_segs)
    a_bin, a_bin_size, _ = build_flat_binary(a_segs, adata, a_min)
    a_size = align_up(a_bin_size, 4096)
    a_end = APP_BASE + a_size

    print(f"Kernel: {k_size} bytes @ 0x{KERNEL_BASE:x}")
    print(f"Root task: {a_size} bytes @ 0x{APP_BASE:x}")

    # Build multiboot1 header (placed at start of trampoline)
    # QEMU only supports Multiboot 1! (magic 0x1BADB002)
    # Flags: don't set bit 16 (address flag) — let QEMU use ELF loading
    mbi1_flags = 0x00000003  # bit 0: page align, bit 1: memory info
    mbi1_checksum = (-(0x1BADB002 + mbi1_flags)) & 0xFFFFFFFF
    mbi = bytearray()
    mbi.extend(p32(0x1BADB002))    # magic
    mbi.extend(p32(mbi1_flags))    # flags
    mbi.extend(p32(mbi1_checksum)) # checksum
    # Pad to keep alignment
    while len(mbi) < 8:
        mbi.append(0)
    mbi = bytes(mbi)

    # The trampoline entry point (after the header)
    trampoline_32_entry_phys = TRAMPOLINE_BASE + len(mbi)

    # Build trampoline  
    trampoline_32_entry_phys = TRAMPOLINE_BASE + len(mbi)
    trampoline = mbi + TRAMPOLINE_32
    trampoline = trampoline.ljust(4096, b"\x00")
    trampoline_size = len(trampoline)

    # === File layout ===
    #   0x0000: ELF32 header + PHDRs
    #   0x1000: trampoline (multiboot2 header + 32-bit boot code)
    #   0x2000: kernel flat binary
    #   ...:    root task flat binary

    trampoline_file_off = 0x1000
    kernel_file_off = trampoline_file_off + trampoline_size
    app_file_off = kernel_file_off + k_size

    # === Build ELF32 output ===
    #
    # IMPORTANT: The multiboot2 header must be at the beginning of the
    # file for QEMU to find it. We place a copy at file offset 0 (before
    # the ELF header) by using a raw prefix approach.
    #
    # Since ELF files must start with \x7fELF, we can't put raw data before
    # the ELF header. Instead, we include the multiboot2 header inside the
    # first LOAD segment (which maps to phys 0x0).
    #
    # QEMU scans the RAW FILE for the multiboot magic, so the header must
    # be in the raw file bytes within the first 32KB.
    #
    # Our file layout after ELF header (148 bytes) has padding to 0x1000,
    # so the multiboot2 header is at raw file offset 0x1000 = 4KB. This
    # should be within QEMU's scan range.

    # === Build ELF32 output ===
    num_phdrs = 3
    e_phoff = ELF32_EHDR_SIZE

    phdrs = bytearray()

    # Trampoline LOAD segment (p_type, p_offset, p_vaddr, p_paddr, p_filesz, p_memsz, p_flags, p_align)
    phdrs.extend(p32(PT_LOAD))
    phdrs.extend(p32(trampoline_file_off))
    phdrs.extend(p32(TRAMPOLINE_BASE))
    phdrs.extend(p32(TRAMPOLINE_BASE))
    phdrs.extend(p32(trampoline_size))
    phdrs.extend(p32(trampoline_size))
    phdrs.extend(p32(PF_R | PF_X))
    phdrs.extend(p32(4096))

    # Kernel LOAD segment
    phdrs.extend(p32(PT_LOAD))
    phdrs.extend(p32(kernel_file_off))
    phdrs.extend(p32(KERNEL_BASE))
    phdrs.extend(p32(KERNEL_BASE))
    phdrs.extend(p32(k_size))
    phdrs.extend(p32(k_size))
    phdrs.extend(p32(PF_R | PF_W | PF_X))
    phdrs.extend(p32(4096))

    # App LOAD segment
    phdrs.extend(p32(PT_LOAD))
    phdrs.extend(p32(app_file_off))
    phdrs.extend(p32(APP_BASE))
    phdrs.extend(p32(APP_BASE))
    phdrs.extend(p32(a_size))
    phdrs.extend(p32(a_size))
    phdrs.extend(p32(PF_R | PF_W))
    phdrs.extend(p32(4096))

    # Build ELF32 header
    elf = bytearray()
    elf.extend(b"\x7fELF")
    elf.append(ELFCLASS32)
    elf.append(ELFDATA2LSB)
    elf.append(EV_CURRENT)
    elf.append(0x00)  # ELFOSABI_NONE
    elf.extend(b"\x00" * 8)
    elf.extend(p16(ET_EXEC))
    elf.extend(p16(EM_386))
    elf.extend(p32(EV_CURRENT))
    elf.extend(p32(trampoline_32_entry_phys))
    elf.extend(p32(e_phoff))
    elf.extend(p32(0))  # e_shoff
    elf.extend(p32(0))  # e_flags
    elf.extend(p16(ELF32_EHDR_SIZE))
    elf.extend(p16(ELF32_PHDR_SIZE))
    elf.extend(p16(num_phdrs))
    elf.extend(p16(0))  # e_shentsize
    elf.extend(p16(0))  # e_shnum
    elf.extend(p16(0))  # e_shstrndx

    # Assemble output
    output = bytearray()
    output.extend(elf)
    output.extend(phdrs)

    # Pad to trampoline
    while len(output) < trampoline_file_off:
        output.append(0)
    output.extend(trampoline)

    # Pad to kernel
    while len(output) < kernel_file_off:
        output.append(0)
    output.extend(k_bin.ljust(k_size, b"\x00"))

    # Pad to app
    while len(output) < app_file_off:
        output.append(0)
    output.extend(a_bin.ljust(a_size, b"\x00"))

    with open(output_path, "wb") as f:
        f.write(output)

    print(f"\nImage: {output_path} ({len(output)/1024:.1f} KiB)")
    print(f"  Entry: 0x{trampoline_32_entry_phys:x}")
    print(f"  Modules: kernel@0x{KERNEL_BASE:x} app@0x{APP_BASE:x}")
    print(f"  Run: qemu-system-x86_64 -kernel {output_path} -nographic")


def main():
    parser = argparse.ArgumentParser(description="Build bootable seL4 QEMU image")
    parser.add_argument("--kernel", required=True)
    parser.add_argument("--app", required=True)
    parser.add_argument("-o", required=True)
    args = parser.parse_args()

    for p in [args.kernel, args.app]:
        if not os.path.exists(p):
            print(f"ERROR: not found: {p}")
            sys.exit(1)

    build_image(args.kernel, args.app, args.o)


if __name__ == "__main__":
    main()
