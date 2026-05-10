#!/usr/bin/env python3
"""
Post-process the ELF image for QEMU 10.0 boot compatibility.

This script:
1. Injects a multiboot1 header within the first 8KB of the ELF
2. Changes e_machine to EM_386 (required by QEMU's multiboot path)
3. Adds a PT_NOTE segment with XEN PVH note (for PVH boot)
4. Produces a raw binary alt-image with proper multiboot header

Usage: python3 add-pvh-note.py <elf_file> <pvh_entry_hex> <elf_entry_hex>
"""

import struct, sys, os

def p32(v): return struct.pack("<I", v)
def p64(v): return struct.pack("<Q", v)

def add_boot_notes(elf_path, entry32):
    with open(elf_path, 'rb') as f:
        data = bytearray(f.read())

    if data[:4] != b'\x7fELF' or data[4] != 2:
        print("ERROR: Not a 64-bit ELF")
        sys.exit(1)

    e_phoff = struct.unpack_from('<Q', data, 32)[0]
    e_phentsize = struct.unpack_from('<H', data, 54)[0]
    e_phnum = struct.unpack_from('<H', data, 56)[0]
    e_entry = struct.unpack_from('<Q', data, 24)[0]
    e_machine = struct.unpack_from('<H', data, 18)[0]

    modified = False
    needs_raw = False

    # ---- Step 1: Inject multiboot1 header within the first 8KB ----
    mbi_data_off = e_phoff + e_phnum * e_phentsize
    mbi_data_off = (mbi_data_off + 3) & ~3

    mb_found = False
    for i in range(0, min(32768, len(data)), 4):
        if struct.unpack_from('<I', data, i)[0] in (0xE85250D6, 0x1BADB002):
            if i < 8192:
                mb_found = True
                break

    if not mb_found and mbi_data_off < 8100:
        flags = 0x00000003
        mbi = struct.pack("<IIIIIIII", 
            0x1BADB002,          # magic
            0x00010003,          # HAS_ADDR | page_align | memory_info
            (-(0x1BADB002 + 0x00010003)) & 0xFFFFFFFF,  # checksum
            0x200000,            # header_addr
            0x200000,            # load_addr
            0x200000 + 0xA00000, # load_end (10 MB)
            0x200000 + 0xA00000, # bss_end
            e_entry & 0xFFFFFFFF # entry (from ELF)
        )
        data[mbi_data_off:mbi_data_off] = mbi
        for i in range(e_phnum):
            off = e_phoff + i * e_phentsize
            if struct.unpack_from('<I', data, off)[0] == 1:  # PT_LOAD
                old = struct.unpack_from('<Q', data, off + 8)[0]
                struct.pack_into('<Q', data, off + 8, old + len(mbi))
        print(f"Embedded multiboot1 header (HAS_ADDR) at offset 0x{mbi_data_off:x}")
        modified = True

    # ---- Step 2: Patch e_machine to EM_386 ----
    if e_machine == 0x3e:  # EM_X86_64
        struct.pack_into('<H', data, 18, 3)  # EM_386
        print(f"Patched e_machine: EM_X86_64 -> EM_386")
        modified = True

    # ---- Step 3: Add PVH PT_NOTE ----
    has_note = False
    for i in range(e_phnum):
        if struct.unpack_from('<I', data, e_phoff + i * e_phentsize)[0] == 4:
            has_note = True; break

    if not has_note:
        note = bytearray()
        note.extend(p32(4)); note.extend(p32(4)); note.extend(p32(18))
        note.extend(b"Xen\x00"); note.extend(p32(entry32 & 0xFFFFFFFF))
        note = bytes(note)
        while len(data) % 4: data.append(0)
        note_offset = len(data)
        data.extend(note)
        phdr = bytearray()
        phdr.extend(p32(4)); phdr.extend(p32(4))
        phdr.extend(p64(note_offset)); phdr.extend(p64(0))
        phdr.extend(p64(0)); phdr.extend(p64(len(note)))
        phdr.extend(p64(len(note))); phdr.extend(p64(4))
        data[e_phoff:e_phoff] = phdr
        struct.pack_into('<H', data, 56, e_phnum + 1)
        print(f"Added PVH note: entry=0x{entry32:x}")
        modified = True

    if modified:
        with open(elf_path, 'wb') as f:
            f.write(data)
        print(f"Patched {elf_path}")

    # ---- Step 4: Create raw multiboot image ----
    # Even with all the patches, QEMU 10.0's ELF multiboot path might fail.
    # Create a backup: flat binary with multiboot header at offset 0.
    raw_path = elf_path.replace('.elf', '.mb.elf')
    # Build raw multiboot ELF32
    # Read the patched ELF's LOAD segments and create a flat layout
    flat = bytearray()
    # Multiboot1 header (32 bytes) with HAS_ADDR
    flat.extend(struct.pack("<IIIIIIII",
        0x1BADB002,            # magic  
        0x00010003,            # HAS_ADDR | page_align | memory_info
        (-(0x1BADB002 + 0x00010003)) & 0xFFFFFFFF,
        0x100000,              # header_addr (at 1MB)
        0x100000,              # load_addr (1MB)
        0x200000,              # load_end (2MB — enough for kernel-loader + payload)
        0x200000,              # bss_end
        entry32 & 0xFFFFFFFF   # entry = pvh_start (32-bit)
    ))

    # Read LOAD segments from patched ELF and append to flat binary
    e_phoff2 = struct.unpack_from('<Q', data, 32)[0]
    e_phnum2 = struct.unpack_from('<H', data, 56)[0]
    load_segs = []
    for i in range(e_phnum2):
        off = e_phoff2 + i * struct.unpack_from('<H', data, 54)[0]
        if struct.unpack_from('<I', data, off)[0] == 1:  # PT_LOAD
            p_offset = struct.unpack_from('<Q', data, off + 8)[0]
            p_filesz = struct.unpack_from('<Q', data, off + 32)[0]
            p_paddr = struct.unpack_from('<Q', data, off + 24)[0]
            load_segs.append((p_offset, p_paddr, p_filesz))

    if load_segs:
        # Sort by paddr, create flat layout starting from first load addr
        load_segs.sort(key=lambda x: x[1])
        base_addr = load_segs[0][1]
        # Pad to base
        while len(flat) < base_addr - 0x100000:
            flat.append(0)
        for p_offset, p_paddr, p_filesz in load_segs:
            dest = p_paddr - base_addr + len(flat) - (base_addr - 0x100000)
            # Actually, simpler: use p_offset directly
            seg_data = data[p_offset : p_offset + p_filesz]
            flat.extend(seg_data)
            # Pad to next
            while len(flat) % 4096:
                flat.append(0)

    flat = bytes(flat)
    with open(raw_path, 'wb') as f:
        f.write(flat)
    print(f"Created raw multiboot image: {raw_path} ({len(flat)} bytes)")
    print(f"  Boot with: qemu-system-x86_64 -kernel {raw_path} -nographic")

if __name__ == '__main__':
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} <elf_file> <pvh_entry_hex>")
        sys.exit(1)
    add_boot_notes(sys.argv[1], int(sys.argv[2], 16))
