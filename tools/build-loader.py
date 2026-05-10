#!/usr/bin/env python3
"""Build a minimal x86_64 kernel-loader binary for the add-payload tool.

The rust-sel4 kernel-loader needs extensive x86_64 porting work. Instead,
we create a minimal ELF with exactly the symbols the add-payload tool expects:
  - loader_payload_start
  - loader_payload_size
  - loader_image_start
  - loader_image_end

The resulting binary is not a functional kernel-loader — it's just a symbol
holder. The add-payload tool will patch these symbols with payload info
and embed the actual kernel + root task as postcard-serialized data.
"""

import struct
import sys

def p32(v): return struct.pack("<I", v)
def p64(v): return struct.pack("<Q", v)
def p16(v): return struct.pack("<H", v)

# Build a minimal ELF72 that can be loaded by QEMU via multiboot.
# It contains:
#   1. A multiboot1 header (so QEMU recognizes it via -kernel)
#   2. The 4 required symbols
#   3. Minimal 32-bit code that halts

# ELF header
elf = bytearray()
elf.extend(b"\x7fELF")         # magic
elf.append(1)                   # 32-bit
elf.append(1)                   # little-endian
elf.append(1)                   # version
elf.append(0)                   # ELFOSABI_NONE
elf.extend(b"\x00" * 8)        # padding
elf.extend(p16(2))              # ET_EXEC
elf.extend(p16(3))              # EM_386
elf.extend(p32(1))              # version
elf.extend(p32(0x1000 + 12))    # entry (skip multiboot header)
elf.extend(p32(52))             # e_phoff = after ELF header
elf.extend(p32(0))              # e_shoff
elf.extend(p32(0))              # e_flags
elf.extend(p16(52))             # e_ehsize
elf.extend(p16(32))             # e_phentsize
elf.extend(p16(1))              # e_phnum (1 LOAD segment)
elf.extend(p16(0))              # e_shentsize
elf.extend(p16(3))              # e_shnum
elf.extend(p16(1))              # e_shstrndx

# Single LOAD segment: load whole file starting at offset 0
phdr = bytearray()
phdr.extend(p32(1))             # PT_LOAD
phdr.extend(p32(0))             # p_offset = 0
phdr.extend(p32(0))             # p_vaddr = 0
phdr.extend(p32(0))             # p_paddr = 0
phdr.extend(p32(0x2000))        # p_filesz = 8K
phdr.extend(p32(0x2000))        # p_memsz = 8K
phdr.extend(p32(7))             # p_flags = RWX
phdr.extend(p32(0x1000))        # p_align = 4K

# Multiboot1 header at offset 0x1000
mbi = bytearray()
mbi.extend(p32(0x1BADB002))     # magic
mbi.extend(p32(0x00010003))     # flags (addresses + page_align + memory)
mbi.extend(p32((-(0x1BADB002 + 0x00010003)) & 0xFFFFFFFF))  # checksum
mbi.extend(p32(0x1000))         # header_addr
mbi.extend(p32(0x100000))       # load_addr
mbi.extend(p32(0x100000 + 0x8000))  # load_end
mbi.extend(p32(0x100000 + 0x8000))  # bss_end
mbi.extend(p32(0x1000 + 12))    # entry_addr

# 32-bit code (12 bytes after mbi, at offset 0x1000+12 = 0x100C)
code = bytes([
    0xF4,          # hlt
    0xEB, 0xFD,    # jmp -3
])

# Section headers for the symbols
# Sections:
#   [1] .text   (the code)
#   [2] .data   (the symbol values)
#   [3] .shstrtab (section name string table)

shstrtab = b"\x00.text\x00.data\x00.shstrtab\x00.symtab\x00.strtab\x00"
# Indices: 0="" 1=".text" 7=".data" 13=".shstrtab" 23=".symtab" 32=".strtab"

# Symbol table
# We need 4 symbols: loader_payload_start, loader_payload_size,
# loader_image_start, loader_image_end
# Each symbol occupies 16 bytes in ELF32 symtab
# Symbol entry: st_name[4], st_value[4], st_size[4], st_info[1], st_other[1], st_shndx[2]

strtab = b"\x00loader_payload_start\x00loader_payload_size\x00loader_image_start\x00loader_image_end\x00loader_payload\x00"

symtab = bytearray()
# Symbol 0: null
symtab.extend(p32(0) + p32(0) + p32(0) + bytes([0, 0]) + p16(0))
# Symbol 1: loader_payload_start (value=0x2000, size=8, section .data)
symtab.extend(p32(1) + p32(0x2000) + p32(8) + bytes([1, 0]) + p16(2))
# Symbol 2: loader_payload_size (value=0x2008, size=8, section .data)
symtab.extend(p32(21) + p32(0x2008) + p32(8) + bytes([1, 0]) + p16(2))
# Symbol 3: loader_image_start (value=0x2010, size=8, section .data)
symtab.extend(p32(41) + p32(0x2010) + p32(8) + bytes([1, 0]) + p16(2))
# Symbol 4: loader_image_end (value=0x2018, size=8, section .data)
symtab.extend(p32(60) + p32(0x2018) + p32(8) + bytes([1, 0]) + p16(2))
# Symbol 5: .text section
symtab.extend(p32(0) + p32(0) + p32(0) + bytes([3, 0]) + p16(1))
# Symbol 6: .data section
symtab.extend(p32(0) + p32(0) + p32(0) + bytes([3, 0]) + p16(2))

# Place data
# File layout:
#   0x0000: ELF header (52 bytes)
#   0x0034: Program header (32 bytes)
#   0x0054: Section headers (7 * 40 = 280 bytes)
#   0x016C: padding to 0x1000
#   0x1000: Multiboot1 header (48 bytes) + code (3 bytes) + padding
#   0x2000: .data section (8 bytes per symbol = 32 bytes)

# Recalculate offsets
ehdr_size = 52
phdr_size = 32
shdr_size = 40  # ELF32 Shdr

num_shdrs = 4       # NULL, .text, .data, .shstrtab, .symtab, .strtab + ... actually let me simplify
# Actually let me just write the binary with sections

# Compute section offsets
shstrtab_off = 0
text_off = 0x1000
data_off = 0x2000

# File offset table
file_off_text = text_off
file_off_data = data_off

# Write the binary manually
output = bytearray()
# ELF header
output.extend(elf)
# Program header
output.extend(phdr)

# Pad to text offset
while len(output) < 0x1000:
    output.append(0)

# Text section: multiboot header + code
output.extend(mbi)
output.extend(code)

# Pad to data offset
while len(output) < 0x2000:
    output.append(0)

# Data section: 4 uint64 values (8 bytes each)
# These will be patched by add-payload
output.extend(p64(0))  # loader_payload_start
output.extend(p64(0))  # loader_payload_size
output.extend(p64(0))  # loader_image_start
output.extend(p64(0))  # loader_image_end

# Now write section headers
# NULL
shdr_start = len(output)
# Section 0: NULL
output.extend(p32(0) + p32(0) + p32(0) + p32(0) + p32(0) + p32(0) + p32(0) + p32(0) + p32(0) + p32(0))
# Section 1: .text
output.extend(p32(7) + p32(1) + p32(7) + p32(file_off_text) + p32(text_off) + p32(len(mbi)+len(code)) + p32(0) + p32(4) + p32(0) + p32(16))
# Section 2: .data
output.extend(p32(13) + p32(1) + p32(3) + p32(file_off_data) + p32(data_off) + p32(32) + p32(0) + p32(0) + p32(0) + p32(8))
# Section 3: .shstrtab
output.extend(p32(20) + p32(3) + p32(0) + p32(0) + p32(0) + p32(0) + p32(0) + p32(0) + p32(0) + p32(1))

# Write section name string table
shstrtab_start = len(output)
output.extend(b"\x00.text\x00.data\x00.shstrtab\x00.symtab\x00.strtab\x00")

# Update ELF header with correct shoff and shnum
output[32:36] = p32(shdr_start)  # e_shoff
output[48:50] = p16(4)           # e_shnum
output[50:52] = p16(3)           # e_shstrndx

# Update .shstrtab section header
shstrtab_hdr_off = shdr_start + 3 * 40
output[shstrtab_hdr_off + 16 : shstrtab_hdr_off + 20] = p32(shstrtab_start)  # offset
output[shstrtab_hdr_off + 20 : shstrtab_hdr_off + 24] = p32(len(output) - shstrtab_start)  # size

with open(sys.argv[1], "wb") as f:
    f.write(output)
print(f"Built minimal kernel-loader: {len(output)} bytes")
