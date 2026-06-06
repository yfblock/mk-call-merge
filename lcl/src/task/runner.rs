//! Task runner - creates and runs seL4 user tasks
//!
//! Creates a seL4 task, loads ELF segments into mapped pages,
//! and starts execution. Uses root task's VSpace for simplicity.

use common::config::PAGE_SIZE;
use sel4_sys::*;

use crate::child_test::TASK_MAP;
use crate::task::Sel4Task;

const CNODE_DEPTH: usize = 64;
const STACK_VADDR: usize = 0xF10000;
const IPC_BUF_VADDR: usize = 0xF11000;
/// Base address where ELF segments are relocated
const LOAD_BASE: usize = 0x400000;

/// Create and start a user task from an ELF binary.
pub fn create_user_task(
    bi: &BootInfo,
    elf_data: &[u8],
    _args: &[&str],
) -> Option<(usize, usize)> {
    if elf_data.len() < 64 || &elf_data[0..4] != b"\x7fELF" || elf_data[4] != 2 {
        seL4_DebugPutString("[runner] Invalid ELF\n");
        return None;
    }

    let e_entry = u64::from_le_bytes(elf_data[24..32].try_into().ok()?) as usize;
    let e_phoff = u64::from_le_bytes(elf_data[32..40].try_into().ok()?) as usize;
    let e_phentsize = u16::from_le_bytes(elf_data[54..56].try_into().ok()?) as usize;
    let e_phnum = u16::from_le_bytes(elf_data[56..58].try_into().ok()?) as usize;

    // Find lowest PT_LOAD vaddr for relocation
    let mut lowest_vaddr = usize::MAX;
    for i in 0..e_phnum {
        let ph_off = e_phoff + i * e_phentsize;
        if ph_off + 56 > elf_data.len() { break; }
        let p_type = u32::from_le_bytes(elf_data[ph_off..ph_off + 4].try_into().ok()?);
        if p_type == 1 {
            let p_vaddr = u64::from_le_bytes(elf_data[ph_off + 16..ph_off + 24].try_into().ok()?) as usize;
            if p_vaddr < lowest_vaddr { lowest_vaddr = p_vaddr; }
        }
    }
    if lowest_vaddr == usize::MAX { return None; }

    let reloc_offset = LOAD_BASE.wrapping_sub(lowest_vaddr);
    let relocated_entry = e_entry.wrapping_add(reloc_offset);

    seL4_DebugPutString("[runner] Entry=0x");
    print_hex(e_entry);
    seL4_DebugPutString(" -> 0x");
    print_hex(relocated_entry);
    seL4_DebugPutChar(b'\n');

    // Count PT_LOAD pages needed
    let mut pages_needed: usize = 0;
    for i in 0..e_phnum {
        let ph_off = e_phoff + i * e_phentsize;
        if ph_off + 56 > elf_data.len() { break; }
        let p_type = u32::from_le_bytes(elf_data[ph_off..ph_off + 4].try_into().ok()?);
        if p_type == 1 {
            let p_memsz = u64::from_le_bytes(elf_data[ph_off + 40..ph_off + 48].try_into().ok()?) as usize;
            pages_needed += (p_memsz + PAGE_SIZE - 1) / PAGE_SIZE;
        }
    }
    pages_needed += 3; // PT for load region, PT for stack region, TCB
    seL4_DebugPutString("[runner] Pages needed: ");
    print_hex(pages_needed);
    seL4_DebugPutChar(b'\n');

    // Find a large untyped (2MB) for frames, and a small one for TCB/PT
    // The 2MB untyped can hold 512 x 4KB frames
    let (big_untyped, big_untyped_size) = bi.find_free_untyped(21)?; // 2MB
    let (small_untyped, _) = bi.find_free_untyped(12)?; // 4KB

    // Get empty slots from bootinfo
    let empty = bi.empty();
    seL4_DebugPutString("[runner] Empty slots: ");
    print_hex(empty.start);
    seL4_DebugPutString("..");
    print_hex(empty.end);
    seL4_DebugPutChar(b'\n');

    let mut next_slot = empty.start;

    // Allocate slots from empty region
    let tcb_slot = next_slot; next_slot += 1;
    let fault_ep_slot = next_slot; next_slot += 1;
    let stack_frame_slot = next_slot; next_slot += 1;
    let ipc_frame_slot = next_slot; next_slot += 1;
    let pt_load_slot = next_slot; next_slot += 1;
    let pt_stack_slot = next_slot; next_slot += 1;

    seL4_DebugPutString("[runner] fault_ep_slot=");
    print_hex(fault_ep_slot);
    seL4_DebugPutString(" tcb_slot=");
    print_hex(tcb_slot);
    seL4_DebugPutChar(b'\n');

    // Update OBJ_ALLOCATOR with remaining slots (add in reverse order so alloc() returns smallest first)
    {
        let mut sm = crate::utils::obj::OBJ_ALLOCATOR.lock();
        for slot in (next_slot..empty.end).rev() {
            sm.extend_slot(slot);
        }
    }

    // Create Endpoint for fault handling
    let err = seL4_Untyped_Retype(
        small_untyped, ObjectType::Endpoint as usize,
        ObjectType::Endpoint.size_bits(), init_slots::CNODE, init_slots::CNODE,
        CNODE_DEPTH, fault_ep_slot, 1,
    );
    if err != 0 {
        seL4_DebugPutString("[runner] Endpoint failed err=");
        print_hex(err as usize);
        seL4_DebugPutChar(b'\n');
        return None;
    }

    // Create TCB
    let err = seL4_Untyped_Retype(
        small_untyped, ObjectType::TCB as usize,
        ObjectType::TCB.size_bits(), init_slots::CNODE, init_slots::CNODE,
        CNODE_DEPTH, tcb_slot, 1,
    );
    if err != 0 { seL4_DebugPutString("[runner] TCB failed\n"); return None; }

    // Create page tables for all 2MB-aligned regions needed by ELF segments
    let pt_load_slot = {
        let mut sm = crate::utils::obj::OBJ_ALLOCATOR.lock();
        sm.alloc()?
    };
    let (pt_untyped, _) = bi.find_free_untyped(12)?;
    let err = seL4_Untyped_Retype(
        pt_untyped, ObjectType::PageTable as usize,
        ObjectType::PageTable.size_bits(), init_slots::CNODE, init_slots::CNODE,
        CNODE_DEPTH, pt_load_slot, 1,
    );
    if err != 0 { seL4_DebugPutString("[runner] PT load retype failed err="); print_hex(err); seL4_DebugPutChar(b'\n'); }
    let err = seL4_PageTable_Map(pt_load_slot, init_slots::VSPACE, LOAD_BASE, 0);
    if err != 0 {
        // Page table might already exist at this address - continue anyway
        seL4_DebugPutString("[runner] PT load map: err=");
        print_hex(err);
        seL4_DebugPutString(" (continuing)\n");
    }

    // Create additional page tables for higher addresses if needed
    // Each page table covers 2MB (0x200000)
    let mut mapped_pts: alloc::vec::Vec<usize> = alloc::vec::Vec::new();
    mapped_pts.push(LOAD_BASE & !0x1FFFFF); // track which 2MB regions have PTs

    for i in 0..e_phnum {
        let ph_off = e_phoff + i * e_phentsize;
        if ph_off + 56 > elf_data.len() { break; }
        let p_type = u32::from_le_bytes(elf_data[ph_off..ph_off + 4].try_into().ok()?);
        if p_type != 1 { continue; }
        let p_vaddr = u64::from_le_bytes(elf_data[ph_off + 16..ph_off + 24].try_into().ok()?) as usize;
        let p_filesz = u64::from_le_bytes(elf_data[ph_off + 32..ph_off + 40].try_into().ok()?) as usize;
        let p_memsz = u64::from_le_bytes(elf_data[ph_off + 40..ph_off + 48].try_into().ok()?) as usize;
        let relocated_start = p_vaddr.wrapping_add(reloc_offset);
        let relocated_end = relocated_start + p_memsz;

        seL4_DebugPutString("[runner] PT_LOAD: 0x");
        print_hex(p_vaddr);
        seL4_DebugPutString(" -> 0x");
        print_hex(relocated_start);
        seL4_DebugPutString(" filesz=0x");
        print_hex(p_filesz);
        seL4_DebugPutString(" memsz=0x");
        print_hex(p_memsz);
        seL4_DebugPutChar(b'\n');

        let pt_region_start = relocated_start & !0x1FFFFF;
        let pt_region_end = (relocated_end + 0x1FFFFF) & !0x1FFFFF;

        for region in (pt_region_start..pt_region_end).step_by(0x200000) {
            if mapped_pts.contains(&region) { continue; }
            let new_pt = { let mut sm = crate::utils::obj::OBJ_ALLOCATOR.lock(); sm.alloc()? };
            let (new_pt_ut, _) = bi.find_free_untyped(12)?;
            let err = seL4_Untyped_Retype(
                new_pt_ut, ObjectType::PageTable as usize,
                ObjectType::PageTable.size_bits(), init_slots::CNODE, init_slots::CNODE,
                CNODE_DEPTH, new_pt, 1,
            );
            if err != 0 { seL4_DebugPutString("[runner] PT extra retype failed\n"); return None; }
            let err = seL4_PageTable_Map(new_pt, init_slots::VSPACE, region, 0);
            if err != 0 {
                // Page table might already exist - continue anyway
                seL4_DebugPutString("[runner] PT map at 0x");
                print_hex(region);
                seL4_DebugPutString(": err=");
                print_hex(err);
                seL4_DebugPutString(" (continuing)\n");
            }
            mapped_pts.push(region);
        }
    }

    // Create and map page table for stack/IPC region
    let (pt_stack_ut, _) = bi.find_free_untyped(12)?;
    let err = seL4_Untyped_Retype(
        pt_stack_ut, ObjectType::PageTable as usize,
        ObjectType::PageTable.size_bits(), init_slots::CNODE, init_slots::CNODE,
        CNODE_DEPTH, pt_stack_slot, 1,
    );
    if err != 0 { seL4_DebugPutString("[runner] PT stack failed\n"); return None; }
    let err = seL4_PageTable_Map(pt_stack_slot, init_slots::VSPACE, 0xF00000, 0);
    if err != 0 { seL4_DebugPutString("[runner] PT stack map failed\n"); return None; }

    // Pre-calculate total ELF frames needed.
    // page_info holds (page_vaddr, file_off, copy_off_in_page, copy_len):
    //   file_off          — byte offset into elf_data to copy from
    //   copy_off_in_page  — byte offset within the page to copy to
    //   copy_len          — number of bytes to copy (0 = pure BSS page)
    let mut total_elf_pages: usize = 0;
    let mut page_info: alloc::vec::Vec<(usize, usize, usize, usize)> = alloc::vec::Vec::new();

    for i in 0..e_phnum {
        let ph_off = e_phoff + i * e_phentsize;
        if ph_off + 56 > elf_data.len() { break; }

        let p_type = u32::from_le_bytes(elf_data[ph_off..ph_off + 4].try_into().ok()?);
        let p_offset = u64::from_le_bytes(elf_data[ph_off + 8..ph_off + 16].try_into().ok()?) as usize;
        let p_vaddr = u64::from_le_bytes(elf_data[ph_off + 16..ph_off + 24].try_into().ok()?) as usize;
        let p_filesz = u64::from_le_bytes(elf_data[ph_off + 32..ph_off + 40].try_into().ok()?) as usize;
        let p_memsz = u64::from_le_bytes(elf_data[ph_off + 40..ph_off + 48].try_into().ok()?) as usize;

        if p_type != 1 { continue; }

        let relocated_vaddr = p_vaddr.wrapping_add(reloc_offset);
        // Segments need not be page-aligned (e.g. the RW data segment starts
        // mid-page). Walk every page the segment touches and copy the slice of
        // file data that falls inside it at the correct in-page offset.
        let seg_start = relocated_vaddr;
        let seg_file_end = relocated_vaddr + p_filesz;   // end of file-backed bytes
        let seg_mem_end = relocated_vaddr + p_memsz;     // end of mapped region (incl. BSS)
        let start_page = seg_start & !(PAGE_SIZE - 1);
        let end_page = (seg_mem_end + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

        seL4_DebugPutString("[runner] PT_LOAD: 0x");
        print_hex(p_vaddr);
        seL4_DebugPutString(" -> 0x");
        print_hex(relocated_vaddr);
        seL4_DebugPutString(" filesz=0x");
        print_hex(p_filesz);
        seL4_DebugPutString(" memsz=0x");
        print_hex(p_memsz);
        seL4_DebugPutChar(b'\n');

        for page_vaddr in (start_page..end_page).step_by(PAGE_SIZE) {
            let page_end = page_vaddr + PAGE_SIZE;
            // Intersect [seg_start, seg_file_end) with [page_vaddr, page_end).
            let copy_vstart = seg_start.max(page_vaddr);
            let copy_vend = seg_file_end.min(page_end);
            let (file_off, copy_off_in_page, copy_len) = if copy_vstart < copy_vend {
                let file_off = p_offset + (copy_vstart - seg_start);
                let copy_off_in_page = copy_vstart - page_vaddr;
                (file_off, copy_off_in_page, copy_vend - copy_vstart)
            } else {
                (0, 0, 0) // page is entirely BSS (zero-fill)
            };
            // Skip pages already recorded by an earlier segment (none overlap
            // for busybox, but guard against double-mapping just in case).
            if page_info.iter().any(|&(pv, _, _, _)| pv == page_vaddr) {
                continue;
            }
            page_info.push((page_vaddr, file_off, copy_off_in_page, copy_len));
            total_elf_pages += 1;
        }
        let _ = seg_file_end;
    }

    seL4_DebugPutString("[runner] Total ELF pages: ");
    print_hex(total_elf_pages);
    seL4_DebugPutChar(b'\n');

    // Allocate frame slots from OBJ_ALLOCATOR
    let mut frame_slots: alloc::vec::Vec<usize> = alloc::vec::Vec::new();
    {
        let mut sm = crate::utils::obj::OBJ_ALLOCATOR.lock();
        for _ in 0..total_elf_pages {
            match sm.alloc() {
                Some(s) => frame_slots.push(s),
                None => { seL4_DebugPutString("[runner] No slots for frames\n"); return None; }
            }
        }
    }

    // Batch-allocate all ELF frames from the big untyped
    // seL4 limits num_objects to CONFIG_RETYPE_FAN_OUT_LIMIT (256)
    const MAX_RETYPE: usize = 256;
    let mut frames_allocated = 0;
    seL4_DebugPutString("[runner] First frame slot: ");
    print_hex(frame_slots[0]);
    seL4_DebugPutString(" total: ");
    print_hex(total_elf_pages);
    seL4_DebugPutChar(b'\n');
    while frames_allocated < total_elf_pages {
        let batch_size = (total_elf_pages - frames_allocated).min(MAX_RETYPE);
        seL4_DebugPutString("[runner] Retype batch: ");
        print_hex(batch_size);
        seL4_DebugPutString(" at slot ");
        print_hex(frame_slots[frames_allocated]);
        seL4_DebugPutChar(b'\n');
        let err = seL4_Untyped_Retype(
            big_untyped, ObjectType::Frame4K as usize,
            ObjectType::Frame4K.size_bits(), init_slots::CNODE, init_slots::CNODE,
            CNODE_DEPTH, frame_slots[frames_allocated], batch_size,
        );
        if err != 0 {
            seL4_DebugPutString("[runner] Batch frame retype failed err=");
            print_hex(err as usize);
            seL4_DebugPutString(" batch=");
            print_hex(batch_size);
            seL4_DebugPutString(" offset=");
            print_hex(frames_allocated);
            seL4_DebugPutChar(b'\n');
            return None;
        }
        frames_allocated += batch_size;
    }

    // Map and populate each frame
    let mut segments_loaded = 0;
    for (idx, &(page_vaddr, file_off, copy_off_in_page, copy_len)) in page_info.iter().enumerate() {
        let frame_slot = frame_slots[idx];

        // Map frame
        let err = seL4_Frame_Map(
            frame_slot, init_slots::VSPACE, page_vaddr,
            CapRights::ALL.bits(), 0,
        );
        if err != 0 {
            seL4_DebugPutString("[runner] Frame map err at 0x");
            print_hex(page_vaddr);
            seL4_DebugPutString(" err=");
            print_hex(err as usize);
            seL4_DebugPutChar(b'\n');
            return None;
        }

        // Zero the whole frame first so BSS and any partial-page gaps are clean.
        unsafe {
            let dest = page_vaddr as *mut u8;
            for i in 0..PAGE_SIZE {
                dest.add(i).write_volatile(0);
            }
        }

        // Copy the file-backed slice into the correct in-page offset.
        if copy_len > 0 && file_off + copy_len <= elf_data.len() {
            let dest = (page_vaddr + copy_off_in_page) as *mut u8;
            unsafe {
                core::ptr::copy_nonoverlapping(
                    elf_data[file_off..file_off + copy_len].as_ptr(), dest, copy_len,
                );
                // NOTE: Do NOT patch syscall instructions. Let the child
                // execute `syscall` directly, generating an UnknownSyscall
                // fault (label 2) the root task emulates.
            }
        }
        segments_loaded += 1;
    }

    seL4_DebugPutString("[runner] Loaded ");
    print_hex(segments_loaded);
    seL4_DebugPutString(" pages\n");

    // Post-load patching: adjust absolute addresses in the entry point code
    // Only patch if the binary is busybox (entry at 0x4038b1)
    // The minimal test binary doesn't need patching
    let entry_offset = e_entry.wrapping_sub(lowest_vaddr);
    if e_entry == 0x4038b1 {
        // busybox binary - patch absolute addresses
        let patch_addr = (LOAD_BASE + entry_offset) as *mut u8;

        // Patch at entry + 0x20: mov $imm,%r8d (41 b8 XX XX XX XX)
        let patch_off_main = 0x20;
        unsafe {
            let imm_ptr = patch_addr.add(patch_off_main + 2) as *mut u32;
            let old_val = core::ptr::read_unaligned(imm_ptr);
            let new_val = old_val.wrapping_add(reloc_offset as u32);
            core::ptr::write_unaligned(imm_ptr, new_val);
            seL4_DebugPutString("[runner] Patched main: 0x");
            print_hex(old_val as usize);
            seL4_DebugPutString(" -> 0x");
            print_hex(new_val as usize);
            seL4_DebugPutChar(b'\n');
        }

        // Patch at entry + 0x26: mov $imm,%ecx (b9 XX XX XX XX)
        let patch_off_init = 0x26;
        unsafe {
            let imm_ptr = patch_addr.add(patch_off_init + 1) as *mut u32;
            let old_val = core::ptr::read_unaligned(imm_ptr);
            let new_val = old_val.wrapping_add(reloc_offset as u32);
            core::ptr::write_unaligned(imm_ptr, new_val);
            seL4_DebugPutString("[runner] Patched init: 0x");
            print_hex(old_val as usize);
            seL4_DebugPutString(" -> 0x");
            print_hex(new_val as usize);
            seL4_DebugPutChar(b'\n');
        }

        // Patch at entry + 0x2b: mov $imm,%edi (bf XX XX XX XX)
        let patch_off_fini = 0x2b;
        unsafe {
            let imm_ptr = patch_addr.add(patch_off_fini + 1) as *mut u32;
            let old_val = core::ptr::read_unaligned(imm_ptr);
            let new_val = old_val.wrapping_add(reloc_offset as u32);
            core::ptr::write_unaligned(imm_ptr, new_val);
            seL4_DebugPutString("[runner] Patched fini: 0x");
            print_hex(old_val as usize);
            seL4_DebugPutString(" -> 0x");
            print_hex(new_val as usize);
            seL4_DebugPutChar(b'\n');
        }
    }

    // Create and map stack frame - use a fresh untyped to avoid offset issues
    let (stack_untyped, _) = bi.find_free_untyped(12)?; // 4KB
    let err = seL4_Untyped_Retype(
        stack_untyped, ObjectType::Frame4K as usize,
        ObjectType::Frame4K.size_bits(), init_slots::CNODE, init_slots::CNODE,
        CNODE_DEPTH, stack_frame_slot, 1,
    );
    if err != 0 {
        seL4_DebugPutString("[runner] Stack frame failed err=");
        print_hex(err as usize);
        seL4_DebugPutChar(b'\n');
        return None;
    }
    let err = seL4_Frame_Map(
        stack_frame_slot, init_slots::VSPACE, STACK_VADDR,
        CapRights::ALL.bits(), 0,
    );
    if err != 0 {
        seL4_DebugPutString("[runner] Stack map failed err=");
        print_hex(err as usize);
        seL4_DebugPutString(" slot=");
        print_hex(stack_frame_slot);
        seL4_DebugPutString(" vaddr=0x");
        print_hex(STACK_VADDR);
        seL4_DebugPutChar(b'\n');
        return None;
    }
    seL4_DebugPutString("[runner] Stack mapped OK at 0x");
    print_hex(STACK_VADDR);
    seL4_DebugPutString(" slot=");
    print_hex(stack_frame_slot);
    seL4_DebugPutChar(b'\n');

    // Create and map IPC buffer frame - use a fresh untyped
    let (ipc_untyped, _) = bi.find_free_untyped(12)?; // 4KB
    let err = seL4_Untyped_Retype(
        ipc_untyped, ObjectType::Frame4K as usize,
        ObjectType::Frame4K.size_bits(), init_slots::CNODE, init_slots::CNODE,
        CNODE_DEPTH, ipc_frame_slot, 1,
    );
    if err != 0 {
        seL4_DebugPutString("[runner] IPC frame failed err=");
        print_hex(err as usize);
        seL4_DebugPutChar(b'\n');
        return None;
    }
    let err = seL4_Frame_Map(
        ipc_frame_slot, init_slots::VSPACE, IPC_BUF_VADDR,
        CapRights::ALL.bits(), 0,
    );
    if err != 0 { seL4_DebugPutString("[runner] IPC map failed\n"); return None; }

    // Configure TCB with fault endpoint
    // The fault endpoint must be in the child's CSpace
    // We'll use the root CNode as the child's CSpace
    // and the fault_ep_slot is already in the root CNode
    let err = seL4_TCB_Configure(
        tcb_slot, fault_ep_slot,
        init_slots::CNODE, 0,
        init_slots::VSPACE,
        IPC_BUF_VADDR, ipc_frame_slot,
    );
    if err != 0 {
        seL4_DebugPutString("[runner] TCB configure failed err=");
        print_hex(err as usize);
        seL4_DebugPutChar(b'\n');
        return None;
    }

    let err = seL4_TCB_SetSchedParams(tcb_slot, init_slots::TCB, 255, 255);
    if err != 0 { seL4_DebugPutString("[runner] Sched params failed\n"); return None; }

    // Zero the entire stack page first
    unsafe {
        let dest = STACK_VADDR as *mut u8;
        for i in 0..PAGE_SIZE {
            dest.add(i).write_volatile(0);
        }
    }

    // Allocate and map a TLS page for the child
    let tls_base: usize = 0x720000;
    let (tls_untyped, _) = bi.find_free_untyped(12)?;
    let tls_frame_slot = {
        let mut sm = crate::utils::obj::OBJ_ALLOCATOR.lock();
        sm.alloc()?
    };
    let err = seL4_Untyped_Retype(
        tls_untyped, ObjectType::Frame4K as usize,
        ObjectType::Frame4K.size_bits(), init_slots::CNODE, init_slots::CNODE,
        CNODE_DEPTH, tls_frame_slot, 1,
    );
    if err != 0 { seL4_DebugPutString("[runner] TLS frame failed\n"); return None; }
    let err = seL4_Frame_Map(
        tls_frame_slot, init_slots::VSPACE, tls_base,
        CapRights::ALL.bits(), 0,
    );
    if err != 0 { seL4_DebugPutString("[runner] TLS map failed\n"); return None; }
    // Zero the TLS page
    unsafe {
        let dest = tls_base as *mut u8;
        for i in 0..PAGE_SIZE {
            dest.add(i).write_volatile(0);
        }
    }
    seL4_DebugPutString("[runner] TLS mapped at 0x");
    print_hex(tls_base);
    seL4_DebugPutChar(b'\n');

    // Allocate and map a trampoline page for wrfsbase injection
    let trampoline_page: usize = 0x730000;
    let (trampoline_untyped, _) = bi.find_free_untyped(12)?;
    let trampoline_frame_slot = {
        let mut sm = crate::utils::obj::OBJ_ALLOCATOR.lock();
        sm.alloc()?
    };
    let err = seL4_Untyped_Retype(
        trampoline_untyped, ObjectType::Frame4K as usize,
        ObjectType::Frame4K.size_bits(), init_slots::CNODE, init_slots::CNODE,
        CNODE_DEPTH, trampoline_frame_slot, 1,
    );
    if err != 0 { seL4_DebugPutString("[runner] Trampoline frame failed\n"); return None; }
    let err = seL4_Frame_Map(
        trampoline_frame_slot, init_slots::VSPACE, trampoline_page,
        CapRights::ALL.bits(), 0,
    );
    if err != 0 { seL4_DebugPutString("[runner] Trampoline map failed\n"); return None; }
    // Zero the trampoline page
    unsafe {
        let dest = trampoline_page as *mut u8;
        for i in 0..PAGE_SIZE {
            dest.add(i).write_volatile(0);
        }
    }
    seL4_DebugPutString("[runner] Trampoline page at 0x");
    print_hex(trampoline_page);
    seL4_DebugPutChar(b'\n');

    // Initialize the TLS area at 0x720000
    // glibc tcbhead_t layout on x86_64:
    //   0x00: tcb (pointer to TCB - itself)
    //   0x08: dtv (dynamic thread vector - 0 for static)
    //   0x10: self (pointer to TCB - itself)
    //   0x18: multiple_threads (0)
    //   0x1c: gscope_flag (0)
    //   0x20: sysinfo (0)
    //   0x28: stack_guard (canary - must be non-zero)
    //   0x30: pointer_guard (must be non-zero)
    unsafe {
        let dest = tls_base as *mut usize;
        // Zero the whole page first
        for i in 0..(4096 / 8) {
            dest.add(i).write_volatile(0);
        }
        // Set critical fields
        dest.add(0).write_volatile(tls_base);           // tcb = itself
        dest.add(2).write_volatile(tls_base);           // self = itself
        dest.add(5).write_volatile(0x4141414141414141);  // stack_guard (canary)
        dest.add(6).write_volatile(0x4242424242424242);  // pointer_guard
    }
    seL4_DebugPutString("[runner] TLS initialized at 0x");
    print_hex(tls_base);
    seL4_DebugPutChar(b'\n');

    // Write a trampoline at the beginning of the stack page:
    // wrfsbase eax       (f3 0f ae d0)  - set FS_BASE from EAX
    // movabs $entry, %rax (48 b8 XX XX XX XX XX XX XX XX) - load entry address
    // jmp *%rax           (ff e0)    - jump to entry
    let trampoline_addr = STACK_VADDR;
    unsafe {
        let dest = trampoline_addr as *mut u8;
        // wrfsbase eax (F3 0F AE D0)
        dest.add(0).write_volatile(0xf3);
        dest.add(1).write_volatile(0x0f);
        dest.add(2).write_volatile(0xae);
        dest.add(3).write_volatile(0xd0);
        // movabs $relocated_entry, %rax
        dest.add(4).write_volatile(0x48);
        dest.add(5).write_volatile(0xb8);
        let entry_bytes = relocated_entry.to_le_bytes();
        for k in 0..8 {
            dest.add(6 + k).write_volatile(entry_bytes[k]);
        }
        // jmp *%rax
        dest.add(14).write_volatile(0xff);
        dest.add(15).write_volatile(0xe0);
    }
    seL4_DebugPutString("[runner] Trampoline at 0x");
    print_hex(trampoline_addr);
    seL4_DebugPutString(" -> 0x");
    print_hex(relocated_entry);
    seL4_DebugPutChar(b'\n');

    // Write Linux x86_64 ABI stack frame for the child task.
    //
    // System V x86-64 requires %rsp (pointing at argc) to be 16-byte
    // aligned at process entry. The data area (strings, AT_RANDOM bytes)
    // lives at the very top of the stack page and grows down; the argc/
    // argv/envp/auxv array is then placed below it at a 16-byte aligned
    // address. CRITICAL: alignment must be applied to the argc address
    // BEFORE writing the array — never decrement rsp after writing argc.

    // --- Data area (top of stack, grows down) ---
    let mut data = STACK_VADDR + PAGE_SIZE;

    // 16 random bytes for AT_RANDOM
    data -= 16;
    let random_bytes_addr = data;
    unsafe {
        let r = random_bytes_addr as *mut u8;
        for i in 0..16 { r.add(i).write_volatile(i as u8 ^ 0xA5); }
    }

    // "x86_64\0" platform string for AT_PLATFORM
    let platform = b"x86_64\0";
    data -= platform.len();
    let platform_addr = data;
    unsafe {
        let s = platform_addr as *mut u8;
        for (i, &b) in platform.iter().enumerate() { s.add(i).write_volatile(b); }
    }

    // "busybox\0" string for argv[0]
    let argv0 = b"busybox\0";
    data -= argv0.len();
    let str_addr = data;
    unsafe {
        let s = str_addr as *mut u8;
        for (i, &b) in argv0.iter().enumerate() { s.add(i).write_volatile(b); }
    }

    // Compute aux vector values
    let at_entry = relocated_entry;
    let at_phdr = LOAD_BASE + e_phoff;  // program headers at base + offset
    let at_phent = e_phentsize;
    let at_phnum = e_phnum;
    let at_pagesz = PAGE_SIZE;

    // auxv pairs (a_type, a_val), AT_NULL terminator added last.
    let auxv: [(usize, usize); 16] = [
        (25, random_bytes_addr), // AT_RANDOM
        (23, 0),                 // AT_SECURE
        (17, 100),               // AT_CLKTCK
        (16, 0),                 // AT_HWCAP
        (15, platform_addr),     // AT_PLATFORM = "x86_64"
        (14, 0),                 // AT_EGID
        (13, 0),                 // AT_GID
        (12, 0),                 // AT_EUID
        (11, 0),                 // AT_UID
        (9, at_entry),           // AT_ENTRY
        (7, 0),                  // AT_BASE (static, not PIE)
        (6, at_pagesz),          // AT_PAGESZ
        (5, at_phnum),           // AT_PHNUM
        (4, at_phent),           // AT_PHENT
        (3, at_phdr),            // AT_PHDR
        (0, 0),                  // AT_NULL terminator
    ];

    // argv[] and envp[] pointers (each NULL-terminated).
    let argv: [usize; 1] = [str_addr];
    let envp: [usize; 0] = [];

    // Total array size: argc(8) + argv ptrs + argv NULL + envp ptrs + envp NULL + auxv.
    let array_words = 1                // argc
        + argv.len() + 1               // argv[] + NULL
        + envp.len() + 1               // envp[] + NULL
        + auxv.len() * 2;              // auxv pairs (AT_NULL already included)
    let array_bytes = array_words * 8;

    // Place argc at a 16-byte aligned address below the data area. Because the
    // SysV ABI requires %rsp (== &argc) to be 16-byte aligned at _start entry,
    // align the BASE — never adjust rsp after writing argc.
    let mut base = data - array_bytes;
    base &= !0xF;
    let rsp = base;

    // Write the array upward from base.
    let mut p = base;
    let mut push = |val: usize| {
        unsafe { (p as *mut usize).write_volatile(val); }
        p += 8;
    };
    push(argv.len());                  // argc
    for &a in &argv { push(a); }       // argv[]
    push(0);                           // argv NULL
    for &e in &envp { push(e); }       // envp[]
    push(0);                           // envp NULL
    for &(t, v) in &auxv {             // auxv[]
        push(t);
        push(v);
    }

    seL4_DebugPutString("[runner] rsp=0x");
    print_hex(rsp);
    seL4_DebugPutString(" argc@0x");
    print_hex(base);
    seL4_DebugPutString(" argv0=0x");
    print_hex(str_addr);
    seL4_DebugPutChar(b'\n');

    // Write initial registers (seL4 x86_64 UserContext layout)
    // Index 0=RIP, 1=RSP, 2=RFLAGS, 3=RAX, 4=RBX, 5=RCX, 6=RDX,
    // 7=RSI, 8=RDI, 9=RBP, 10=R8, 11=R9, 12=R10, 13=R11, 14=R12, 15=R13, 16=R14, 17=R15
    // Start at the trampoline which does: wrfsbase eax; jmp entry
    // RAX = TLS base so wrfsbase sets FS_BASE correctly
    let regs: [usize; 18] = [
        trampoline_addr,  // 0: RIP = trampoline (wrfsbase eax; jmp entry)
        rsp,              // 1: RSP
        0x202,            // 2: RFLAGS
        tls_base,         // 3: RAX = TLS base for wrfsbase
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    // Use seL4_TCB_WriteRegisters to set registers and resume the task
    let err = seL4_TCB_WriteRegisters(tcb_slot, true, 0, 18, &regs);
    if err != 0 {
        seL4_DebugPutString("[runner] WriteRegs failed err=");
        print_hex(err as usize);
        seL4_DebugPutChar(b'\n');
        return None;
    }

    seL4_DebugPutString("[runner] Task started!\n");
    Some((fault_ep_slot, tcb_slot))
}

fn print_hex(val: usize) {
    for i in (0..16).rev() {
        let nibble = (val >> (i * 4)) & 0xf;
        let c = if nibble < 10 { b'0' + nibble as u8 } else { b'a' + (nibble - 10) as u8 };
        seL4_DebugPutChar(c);
    }
}
