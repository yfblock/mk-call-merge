#!/usr/bin/env python3
"""Build a Linux bzImage from an ELF64 file for QEMU 10.0 boot.
Clones the setup header from the system's Linux kernel and appends
our payload as the protected-mode kernel."""

import struct, sys, glob

def build(elf_path, out_path):
    with open(elf_path, 'rb') as f:
        data = f.read()

    if data[:4] != b'\x7fELF' or data[4] != 2:
        print("ERROR: not ELF64"); sys.exit(1)

    e_phoff = struct.unpack_from('<Q', data, 32)[0]
    e_phentsize = struct.unpack_from('<H', data, 54)[0]
    e_phnum = struct.unpack_from('<H', data, 56)[0]
    e_entry = struct.unpack_from('<Q', data, 24)[0]

    # Extract LOAD segments into flat binary
    segs = []
    for i in range(e_phnum):
        off = e_phoff + i * e_phentsize
        if struct.unpack_from('<I', data, off)[0] == 1:
            segs.append((
                struct.unpack_from('<Q', data, off + 8)[0],
                struct.unpack_from('<Q', data, off + 24)[0],
                struct.unpack_from('<Q', data, off + 32)[0],
                struct.unpack_from('<Q', data, off + 40)[0],
            ))
    if not segs:
        print("ERROR: no LOAD segments"); sys.exit(1)

    base = min(s[1] for s in segs)
    kernel_bin = bytearray()
    for off, paddr, filesz, memsz in sorted(segs, key=lambda s: s[1]):
        dest = paddr - base
        while len(kernel_bin) < dest:
            kernel_bin.append(0)
        kernel_bin.extend(data[off:off+filesz])
        while len(kernel_bin) < dest + memsz:
            kernel_bin.append(0)
    kernel_bin = bytes(kernel_bin)

    # Clone setup header from system's Linux kernel
    kernels = sorted(glob.glob('/boot/vmlinuz-*'))
    if not kernels:
        print("ERROR: No Linux kernel found in /boot"); sys.exit(1)
    ref_kernel = kernels[0]
    print(f"Cloning header: {ref_kernel}")

    with open(ref_kernel, 'rb') as f:
        ref = f.read()
    setup_sects = ref[0x1F1]
    setup_size = (setup_sects + 1) * 512
    setup = bytearray(ref[:setup_size])

    # Update header fields for our kernel
    total_size = len(kernel_bin)
    struct.pack_into('<I', setup, 0x1F4, (total_size + 15) // 16)  # syssize
    # Calculate entry point offset within the first segment
    # The ELF entry is at virtual address e_entry (e.g. 0xa5900c)
    # The first LOAD segment base is at the lowest p_paddr
    first_seg_start = base  # lowest p_paddr (e.g. 0xa52000)
    entry_offset = e_entry - first_seg_start  # offset in flat binary
    code32_entry = 0x100000 + entry_offset     # physical address after loading
    struct.pack_into('<I', setup, 0x214, code32_entry)  # code32_start
    struct.pack_into('<I', setup, 0x228, 0)                         # cmdline
    struct.pack_into('<I', setup, 0x218, 0)                         # ramdisk
    struct.pack_into('<I', setup, 0x21C, 0)                         # ramdisk_size

    # Assemble: setup + kernel data
    output = bytearray(setup)
    output.extend(kernel_bin)

    with open(out_path, 'wb') as f:
        f.write(output)

    print(f"bzImage: {out_path} ({len(output)//1024} KiB)")
    print(f"  ElF entry: 0x{e_entry:x}")
    print(f"  Boot: qemu-system-x86_64 -kernel {out_path} -nographic -serial mon:stdio")

if __name__ == '__main__':
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <elf> <output>"); sys.exit(1)
    build(sys.argv[1], sys.argv[2])
