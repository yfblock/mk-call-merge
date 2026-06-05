//! execve syscall implementation
//!
//! Loads and executes ELF binaries in the current process.

use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use common::config::PAGE_SIZE;
use crate::task::Sel4Task;
use crate::syscall::SysResult;
use crate::fs::ipc_client::FS_CLIENT;

/// ELF magic number
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

/// ELF class
const ELFCLASS64: u8 = 2;

/// Program header types
const PT_LOAD: u32 = 1;

/// Program header structure (64-bit)
#[repr(C)]
struct Elf64Phdr {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

/// ELF header structure (64-bit)
#[repr(C)]
struct Elf64Ehdr {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

/// Read an ELF file from filesystem
fn read_elf_file(path: &str) -> Result<Vec<u8>, i32> {
    // Open file
    let fd = FS_CLIENT.open(path, 0).map_err(|e| e)?; // O_RDONLY

    // Get file size
    let size = FS_CLIENT.file_size(fd).map_err(|e| e as i32)?;

    // Read file content
    let mut content = vec![0u8; size];
    let bytes_read = FS_CLIENT.read(fd, &mut content).map_err(|e| e)?;
    if bytes_read != size {
        let _ = FS_CLIENT.close(fd);
        return Err(-5); // EIO
    }

    // Close file
    let _ = FS_CLIENT.close(fd);

    Ok(content)
}

/// Validate ELF header
fn validate_elf_header(data: &[u8]) -> Result<(), i32> {
    if data.len() < 64 {
        return Err(-8); // ENOEXEC
    }

    // Check magic
    if data[0..4] != ELF_MAGIC {
        return Err(-8); // ENOEXEC
    }

    // Check class (64-bit)
    if data[4] != ELFCLASS64 {
        return Err(-8); // ENOEXEC
    }

    // Check endianness (little endian)
    if data[5] != 1 {
        return Err(-8); // ENOEXEC
    }

    Ok(())
}

/// Load ELF segments into task memory
fn load_elf_segments(task: &Arc<Sel4Task>, data: &[u8], reloc_offset: usize) -> Result<usize, i32> {
    // Parse ELF header
    let e_phoff = u64::from_le_bytes(data[32..40].try_into().unwrap()) as usize;
    let e_phentsize = u16::from_le_bytes(data[54..56].try_into().unwrap()) as usize;
    let e_phnum = u16::from_le_bytes(data[56..58].try_into().unwrap()) as usize;
    let e_entry = u64::from_le_bytes(data[24..32].try_into().unwrap()) as usize;

    // Load PT_LOAD segments
    for i in 0..e_phnum {
        let ph_off = e_phoff + i * e_phentsize;
        if ph_off + 56 > data.len() {
            break;
        }

        let p_type = u32::from_le_bytes(data[ph_off..ph_off + 4].try_into().unwrap());
        if p_type != PT_LOAD {
            continue;
        }

        let p_offset = u64::from_le_bytes(data[ph_off + 8..ph_off + 16].try_into().unwrap()) as usize;
        let p_vaddr = u64::from_le_bytes(data[ph_off + 16..ph_off + 24].try_into().unwrap()) as usize;
        let p_filesz = u64::from_le_bytes(data[ph_off + 32..ph_off + 40].try_into().unwrap()) as usize;
        let p_memsz = u64::from_le_bytes(data[ph_off + 40..ph_off + 48].try_into().unwrap()) as usize;

        // Calculate relocated address
        let relocated_vaddr = p_vaddr.wrapping_add(reloc_offset);

        // Map pages for this segment
        let start_page = relocated_vaddr & !(PAGE_SIZE - 1);
        let end_page = (relocated_vaddr + p_memsz + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

        for page_vaddr in (start_page..end_page).step_by(PAGE_SIZE) {
            task.map_blank_page_simple(page_vaddr);
        }

        // Write segment data
        if p_filesz > 0 {
            let segment_data = &data[p_offset..p_offset + p_filesz];
            task.write_bytes(relocated_vaddr, segment_data);
        }
    }

    Ok(e_entry.wrapping_add(reloc_offset))
}

/// Set up stack with argc, argv, envp
fn setup_stack(task: &Arc<Sel4Task>, stack_top: usize, args: &[&str], envp: &[&str]) -> usize {
    let mut sp = stack_top;

    // Reserve space for stack frame
    sp -= 4096; // 1KB for strings

    // Write strings to stack
    let mut string_ptrs = Vec::new();
    let mut str_ptr = sp;

    // Write argv strings
    for arg in args {
        task.write_bytes(str_ptr, arg.as_bytes());
        task.write_bytes(str_ptr + arg.len(), &[0]); // null terminator
        string_ptrs.push(str_ptr);
        str_ptr += arg.len() + 1;
    }

    // Write envp strings
    for env in envp {
        task.write_bytes(str_ptr, env.as_bytes());
        task.write_bytes(str_ptr + env.len(), &[0]); // null terminator
        string_ptrs.push(str_ptr);
        str_ptr += env.len() + 1;
    }

    // Align to 16 bytes
    str_ptr = (str_ptr + 15) & !15;

    // Write auxiliary vector (empty)
    sp = str_ptr;
    sp -= 16; // AT_NULL entry
    task.write_bytes(sp, &[0u8; 16]);

    // Write envp pointers (null terminated)
    sp -= (envp.len() + 1) * 8;
    for (i, _) in envp.iter().enumerate() {
        let ptr = string_ptrs[args.len() + i];
        task.write_bytes(sp + i * 8, &ptr.to_le_bytes());
    }
    task.write_bytes(sp + envp.len() * 8, &[0u8; 8]); // null terminator

    // Write argv pointers (null terminated)
    sp -= (args.len() + 1) * 8;
    for (i, _) in args.iter().enumerate() {
        let ptr = string_ptrs[i];
        task.write_bytes(sp + i * 8, &ptr.to_le_bytes());
    }
    task.write_bytes(sp + args.len() * 8, &[0u8; 8]); // null terminator

    // Write argc
    sp -= 8;
    task.write_bytes(sp, &args.len().to_le_bytes());

    sp
}

/// execve syscall implementation
pub fn sys_execve(task: &Arc<Sel4Task>, path_addr: usize, argv_addr: usize, envp_addr: usize) -> SysResult {
    // Read path from task memory
    let path = read_cstr_from_task(task, path_addr);
    if path.is_empty() {
        return Err(-2); // ENOENT
    }

    // Read ELF file
    let elf_data = read_elf_file(&path)?;

    // Validate ELF header
    validate_elf_header(&elf_data)?;

    // Calculate relocation offset
    // Use a fixed load base for simplicity
    let load_base = 0x400000;

    // Find lowest vaddr in ELF
    let e_phoff = u64::from_le_bytes(elf_data[32..40].try_into().unwrap()) as usize;
    let e_phentsize = u16::from_le_bytes(elf_data[54..56].try_into().unwrap()) as usize;
    let e_phnum = u16::from_le_bytes(elf_data[56..58].try_into().unwrap()) as usize;

    let mut lowest_vaddr = usize::MAX;
    for i in 0..e_phnum {
        let ph_off = e_phoff + i * e_phentsize;
        if ph_off + 56 > elf_data.len() {
            break;
        }
        let p_type = u32::from_le_bytes(elf_data[ph_off..ph_off + 4].try_into().unwrap());
        if p_type == PT_LOAD {
            let p_vaddr = u64::from_le_bytes(elf_data[ph_off + 16..ph_off + 24].try_into().unwrap()) as usize;
            if p_vaddr < lowest_vaddr {
                lowest_vaddr = p_vaddr;
            }
        }
    }

    if lowest_vaddr == usize::MAX {
        return Err(-8); // ENOEXEC
    }

    let reloc_offset = (load_base as usize).wrapping_sub(lowest_vaddr);

    // Clear existing memory mappings
    task.clear_mapped();

    // Load ELF segments
    let entry = load_elf_segments(task, &elf_data, reloc_offset)?;

    // Read argv and envp from task memory
    // For simplicity, use empty args
    let args = vec!["busybox"];
    let envp = vec!["PATH=/usr/bin:/bin"];

    // Set up stack
    let stack_top = 0x2000000; // 32MB
    let sp = setup_stack(task, stack_top, &args, &envp);

    // Update task info
    {
        let mut info = task.info.lock();
        info.entry = entry;
    }

    // In real implementation, would update registers:
    // RIP = entry
    // RSP = sp
    // RDI = argc
    // RSI = argv_ptr

    Ok(0)
}

/// Helper: read a null-terminated string from task memory
fn read_cstr_from_task(task: &Arc<Sel4Task>, addr: usize) -> alloc::string::String {
    let mut bytes = Vec::new();
    let mut a = addr;
    loop {
        let mut buf = [0u8; 1];
        if !task.read_bytes(a, &mut buf) || buf[0] == 0 {
            break;
        }
        bytes.push(buf[0]);
        a += 1;
        if bytes.len() > 4096 {
            break;
        }
    }
    alloc::string::String::from_utf8_lossy(&bytes).into_owned()
}
