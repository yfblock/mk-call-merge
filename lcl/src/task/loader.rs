//! Busybox ELF loader — loads the binary into memory and creates
//! a dedicated CSpace + VSpace for the child task.
//!
//! Step 1: Parse ELF, allocate frames, copy segments to memory.
//! Step 2: Create a CNode to serve as the child's CSpace.
//! Step 3: Create a PML4 + lower page tables as the child's VSpace,
//!         mapping the pre-loaded frames at the correct virtual addresses.

extern crate alloc;

use common::config::PAGE_SIZE;
use sel4_sys::*;

use crate::utils::obj::OBJ_ALLOCATOR;

/// Base address for ELF segments (matches typical ET_EXEC load address).
const LOAD_BASE: usize = 0x400000;

/// Stack virtual address for the child task.
const STACK_VADDR: usize = 0xF10000;
const STACK_PAGES: usize = 4; // 16KB stack

/// IPC buffer virtual address.
const IPC_BUF_VADDR: usize = 0xF11000;

/// TLS base address.
const TLS_BASE: usize = 0x720000;

/// Number of bits for the child CNode (4096 slots).
const CHILD_CNODE_BITS: usize = 12;
const CHILD_CNODE_SLOTS: usize = 1 << CHILD_CNODE_BITS;

/// Parsed ELF program header info.
#[derive(Debug)]
pub struct ElfSegment {
    pub vaddr: usize,
    pub memsz: usize,
    pub filesz: usize,
    pub file_offset: usize,
    pub flags: u32, // PF_R=1, PF_W=2, PF_X=4
}

/// Information about a loaded ELF.
pub struct LoadedElf {
    pub entry: usize,
    pub segments: alloc::vec::Vec<ElfSegment>,
}

/// Parse an ELF binary and extract PT_LOAD segment information.
pub fn parse_elf(elf_data: &[u8]) -> Option<LoadedElf> {
    if elf_data.len() < 64 || &elf_data[0..4] != b"\x7fELF" || elf_data[4] != 2 {
        seL4_DebugPutString("[loader] Invalid ELF header\n");
        return None;
    }

    let e_entry = u64::from_le_bytes(elf_data[24..32].try_into().ok()?) as usize;
    let e_phoff = u64::from_le_bytes(elf_data[32..40].try_into().ok()?) as usize;
    let e_phentsize = u16::from_le_bytes(elf_data[54..56].try_into().ok()?) as usize;
    let e_phnum = u16::from_le_bytes(elf_data[56..58].try_into().ok()?) as usize;

    let mut segments = alloc::vec::Vec::new();

    for i in 0..e_phnum {
        let ph_off = e_phoff + i * e_phentsize;
        if ph_off + 56 > elf_data.len() {
            break;
        }
        let p_type = u32::from_le_bytes(elf_data[ph_off..ph_off + 4].try_into().ok()?);
        if p_type != 1 {
            continue; // PT_LOAD only
        }
        let p_offset = u64::from_le_bytes(elf_data[ph_off + 8..ph_off + 16].try_into().ok()?) as usize;
        let p_vaddr = u64::from_le_bytes(elf_data[ph_off + 16..ph_off + 24].try_into().ok()?) as usize;
        let p_filesz = u64::from_le_bytes(elf_data[ph_off + 32..ph_off + 40].try_into().ok()?) as usize;
        let p_memsz = u64::from_le_bytes(elf_data[ph_off + 40..ph_off + 48].try_into().ok()?) as usize;
        let p_flags = u32::from_le_bytes(elf_data[ph_off + 4..ph_off + 8].try_into().ok()?);

        segments.push(ElfSegment {
            vaddr: p_vaddr,
            memsz: p_memsz,
            filesz: p_filesz,
            file_offset: p_offset,
            flags: p_flags,
        });
    }

    Some(LoadedElf {
        entry: e_entry,
        segments,
    })
}

/// Holds frame slot numbers for loaded pages, keyed by virtual address.
pub struct LoadedPages {
    pub frames: alloc::collections::BTreeMap<usize, usize>,
}

/// Compute the page-aligned start and end for a segment.
fn page_range(vaddr: usize, size: usize) -> (usize, usize) {
    let start = vaddr & !(PAGE_SIZE - 1);
    let end = (vaddr + size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    (start, end)
}

/// Step 1: Load ELF segments into memory.
///
/// Allocates 4KB frames from untyped memory, maps them into the current
/// (root) VSpace at the ELF's virtual addresses, copies segment data.
///
/// Returns a `LoadedPages` map (vaddr → frame_slot) so the caller can
/// later re-map these frames into a child VSpace.
pub fn load_elf_to_memory(
    bi: &BootInfo,
    elf_data: &[u8],
    loaded: &LoadedElf,
) -> Option<LoadedPages> {
    let mut page_map = alloc::collections::BTreeMap::new();
    let mut total_pages: usize = 0;

    for seg in &loaded.segments {
        let (start_page, end_page) = page_range(seg.vaddr, seg.memsz);
        total_pages += (end_page - start_page) / PAGE_SIZE;
    }

    seL4_DebugPutString("[loader] Need ");
    print_hex(total_pages);
    seL4_DebugPutString(" pages for ELF segments\n");

    // Allocate frame slots from OBJ_ALLOCATOR
    let mut frame_slots: alloc::vec::Vec<usize> = alloc::vec::Vec::new();
    {
        let mut sm = OBJ_ALLOCATOR.lock();
        for _ in 0..total_pages {
            let slot = sm.alloc()?;
            frame_slots.push(slot);
        }
    }

    seL4_DebugPutString("[loader] Allocated ");
    print_hex(frame_slots.len());
    seL4_DebugPutString(" frame slots\n");

    // Pre-compute valid untyped objects: collect all non-device descriptors
    // with their slot numbers, sort by size DESC (largest first). The kernel
    // may have consumed early slots during root-task loading; largest ones
    // are most likely still valid.
    let ut_start = bi.untyped_start();
    let ut_count = bi.untyped_count();
    let mut untyped_list: alloc::vec::Vec<(usize, u8)> = alloc::vec::Vec::new();

    for i in 0..ut_count {
        let desc = bi.untyped_desc(i);
        if desc.is_device == 0 && desc.size_bits >= 12 {
            untyped_list.push((ut_start + i, desc.size_bits));
        }
    }
    // Sort by size descending: largest first
    untyped_list.sort_by(|a, b| b.1.cmp(&a.1));

    seL4_DebugPutString("[loader] Found ");
    print_hex(untyped_list.len());
    seL4_DebugPutString(" untyped objects, largest size_bits=");
    if let Some((_, sz)) = untyped_list.first() {
        print_hex(*sz as usize);
    }
    seL4_DebugPutChar(b'\n');

    // Retype frames one at a time, draining each untyped completely.
    // The seL4 kernel places the child untyped BACK into the source slot,
    // so we can keep using the same slot for multiple retypes.
    let mut done = 0;
    let mut ut_idx = 0;

    while done < total_pages && ut_idx < untyped_list.len() {
        let (ut_slot, size_bits) = untyped_list[ut_idx];
        ut_idx += 1;
        let max_from_this = 1usize << (size_bits.saturating_sub(12));

        for _ in 0..max_from_this {
            if done >= total_pages { break; }
            let err = seL4_Untyped_Retype(
                ut_slot, ObjectType::Frame4K as usize, 12,
                init_slots::CNODE, init_slots::CNODE,
                64, frame_slots[done], 1,
            );
            if err != 0 { break; }
            done += 1;
        }
    }

    if done < total_pages {
        seL4_DebugPutString("[loader] Out of untyped: got ");
        print_hex(done);
        seL4_DebugPutString(" of ");
        print_hex(total_pages);
        seL4_DebugPutString(" frames from ");
        print_hex(ut_idx);
        seL4_DebugPutString(" untyped objects\n");
        return None;
    }

    seL4_DebugPutString("[loader] Retyped all ");
    print_hex(total_pages);
    seL4_DebugPutString(" frames\n");

    // Map and populate each frame.
    // Use full rights for initial mapping so we can copy data; child VSpace
    // will set appropriate permissions when re-mapping.
    let mut idx = 0;
    for seg in &loaded.segments {
        let (start_page, end_page) = page_range(seg.vaddr, seg.memsz);

        for page_vaddr in (start_page..end_page).step_by(PAGE_SIZE) {
            let frame_slot = frame_slots[idx];
            idx += 1;

            // Map with full rights for data copy
            let err = seL4_Frame_Map(
                frame_slot, init_slots::VSPACE, page_vaddr,
                CapRights::ALL.bits(), 0,
            );
            if err != 0 {
                seL4_DebugPutString("[loader] Map failed at 0x");
                print_hex(page_vaddr);
                seL4_DebugPutString(" err=");
                print_hex(err as usize);
                seL4_DebugPutChar(b'\n');
                return None;
            }

            // Copy ELF data (or zero for BSS pages)
            let offset_in_seg = page_vaddr - seg.vaddr;
            let data_start = if offset_in_seg < seg.filesz {
                seg.file_offset + offset_in_seg
            } else {
                0
            };
            let data_end = if offset_in_seg < seg.filesz {
                (data_start + PAGE_SIZE).min(seg.file_offset + seg.filesz)
            } else {
                0
            };
            if data_start < data_end && data_end <= elf_data.len() {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        elf_data[data_start..data_end].as_ptr(),
                        page_vaddr as *mut u8,
                        data_end - data_start,
                    );
                }
            }
            // Zero remaining bytes in the page (BSS or partial)
            let copy_end = if data_start < data_end {
                data_end - data_start
            } else {
                0
            };
            if copy_end < PAGE_SIZE {
                unsafe {
                    let dest = (page_vaddr + copy_end) as *mut u8;
                    for i in 0..(PAGE_SIZE - copy_end) {
                        dest.add(i).write_volatile(0);
                    }
                }
            }

            page_map.insert(page_vaddr, frame_slot);
        }
    }

    seL4_DebugPutString("[loader] Loaded ");
    print_hex(idx);
    seL4_DebugPutString(" pages\n");

    Some(LoadedPages { frames: page_map })
}

/// Step 2: Create a new CNode to serve as the child's CSpace.
///
/// Returns the slot of the new CNode.
pub fn create_child_cspace(bi: &BootInfo) -> Option<usize> {
    let cnode_slot = { OBJ_ALLOCATOR.lock().alloc()? };
    let (ut, _) = bi.find_free_untyped(CHILD_CNODE_BITS as u8)?;

    let err = seL4_Untyped_Retype(
        ut,
        ObjectType::CNode as usize,
        CHILD_CNODE_BITS,
        init_slots::CNODE,
        init_slots::CNODE,
        64,
        cnode_slot,
        1,
    );
    if err != 0 {
        seL4_DebugPutString("[loader] CNode retype failed err=");
        print_hex(err as usize);
        seL4_DebugPutChar(b'\n');
        return None;
    }

    seL4_DebugPutString("[loader] Child CSpace CNode at slot ");
    print_hex(cnode_slot);
    seL4_DebugPutString(" (");
    print_hex(CHILD_CNODE_SLOTS);
    seL4_DebugPutString(" slots)\n");

    Some(cnode_slot)
}

/// Step 3: Create a dedicated VSpace (PML4 + lower page tables) for the child,
/// and map the pre-loaded frames into it.
///
/// Returns the slot of the child's PML4 (VSpace root).
pub fn create_child_vspace(
    bi: &BootInfo,
    loaded_pages: &LoadedPages,
) -> Option<usize> {
    // Create PML4 (VSpace root)
    let pml4_slot = { OBJ_ALLOCATOR.lock().alloc()? };
    let (ut, _) = bi.find_free_untyped(12)?;
    let err = seL4_Untyped_Retype(
        ut,
        ObjectType::PML4 as usize,
        12,
        init_slots::CNODE,
        init_slots::CNODE,
        64,
        pml4_slot,
        1,
    );
    if err != 0 {
        seL4_DebugPutString("[loader] PML4 retype failed err=");
        print_hex(err as usize);
        seL4_DebugPutChar(b'\n');
        return None;
    }
    // NOTE: x86_64 VSpace requires explicit creation of ALL intermediate
    // page table levels: PML4 → PDPT → PageDirectory → PageTable → Frame.
    // seL4_PageTable_Map does NOT auto-create PDPT or PageDirectory.

    // Create PDPT (covers first 512GB, only need PDPT[0] for addresses < 1GB)
    let pdpt_slot = { OBJ_ALLOCATOR.lock().alloc()? };
    let (ut, _) = bi.find_free_untyped(12)?;
    let err = seL4_Untyped_Retype(ut, ObjectType::PDPT as usize, 12,
        init_slots::CNODE, init_slots::CNODE, 64, pdpt_slot, 1);
    if err != 0 { seL4_DebugPutString("[loader] PDPT retype err\n"); return None; }
    let err = seL4_PDPT_Map(pdpt_slot, pml4_slot, 0, 0);
    if err != 0 { seL4_DebugPutString("[loader] PDPT map err="); print_hex(err as usize); seL4_DebugPutChar(b'\n'); return None; }

    // Collect all distinct 2MB regions and 1GB regions needed
    let mut pd_regions: alloc::vec::Vec<usize> = alloc::vec::Vec::new();
    let mut pt_regions: alloc::vec::Vec<usize> = alloc::vec::Vec::new();
    for &vaddr in loaded_pages.frames.keys() {
        let pd_region = vaddr & !0x3FFFFFFF; // 1GB aligned (PD index)
        if !pd_regions.contains(&pd_region) { pd_regions.push(pd_region); }
        let pt_region = vaddr & !0x1FFFFF; // 2MB aligned (PT index)
        if !pt_regions.contains(&pt_region) { pt_regions.push(pt_region); }
    }
    pd_regions.sort();
    pt_regions.sort();

    // Create and map PageDirectory for each 1GB region
    for &region in &pd_regions {
        let pd_slot = { OBJ_ALLOCATOR.lock().alloc()? };
        let (ut, _) = bi.find_free_untyped(12)?;
        let err = seL4_Untyped_Retype(ut, ObjectType::PageDirectory as usize, 12,
            init_slots::CNODE, init_slots::CNODE, 64, pd_slot, 1);
        if err != 0 { seL4_DebugPutString("[loader] PD retype err\n"); return None; }
        let err = seL4_PageDirectory_Map(pd_slot, pml4_slot, region, 0);
        if err != 0 { seL4_DebugPutString("[loader] PD map err\n"); return None; }
    }

    // Create and map PageTable for each 2MB region
    for &region in &pt_regions {
        let pt_slot = { OBJ_ALLOCATOR.lock().alloc()? };
        let (ut, _) = bi.find_free_untyped(12)?;
        let err = seL4_Untyped_Retype(ut, ObjectType::PageTable as usize, 12,
            init_slots::CNODE, init_slots::CNODE, 64, pt_slot, 1);
        if err != 0 { seL4_DebugPutString("[loader] PT retype err\n"); return None; }
        let err = seL4_PageTable_Map(pt_slot, pml4_slot, region, 0);
        if err != 0 { seL4_DebugPutString("[loader] PT map err\n"); return None; }
    }

    // Map all loaded frames into the child VSpace
    for (&vaddr, &frame_slot) in &loaded_pages.frames {
        let cap_rights = CapRights::ALL.bits(); // full access for now
        let err = seL4_Frame_Map(frame_slot, pml4_slot, vaddr, cap_rights, 0);
        if err != 0 {
            seL4_DebugPutString("[loader] Frame map at 0x");
            print_hex(vaddr);
            seL4_DebugPutString(" failed err=");
            print_hex(err as usize);
            seL4_DebugPutChar(b'\n');
            // Might already be mapped from step 1; continue
        }
    }

    // Map stack pages
    for i in 0..STACK_PAGES {
        let stack_vaddr = STACK_VADDR + i * PAGE_SIZE;
        let frame_slot = { OBJ_ALLOCATOR.lock().alloc()? };
        let (ut, _) = bi.find_free_untyped(12)?;
        let err = seL4_Untyped_Retype(
            ut,
            ObjectType::Frame4K as usize,
            12,
            init_slots::CNODE,
            init_slots::CNODE,
            64,
            frame_slot,
            1,
        );
        if err != 0 { return None; }
        let err = seL4_Frame_Map(frame_slot, pml4_slot, stack_vaddr, CapRights::ALL.bits(), 0);
        if err != 0 { return None; }
        // Zero stack
        unsafe {
            let dest = stack_vaddr as *mut u8;
            for j in 0..PAGE_SIZE {
                dest.add(j).write_volatile(0);
            }
        }
    }
    // Stack mapped silently (root .rodata may be unmapped)
    print_hex(STACK_VADDR);
    seL4_DebugPutString(" (");
    print_hex(STACK_PAGES);
    seL4_DebugPutString(" pages)\n");

    // Map TLS page
    {
        let tls_frame_slot = { OBJ_ALLOCATOR.lock().alloc()? };
        let (ut, _) = bi.find_free_untyped(12)?;
        let err = seL4_Untyped_Retype(
            ut,
            ObjectType::Frame4K as usize,
            12,
            init_slots::CNODE,
            init_slots::CNODE,
            64,
            tls_frame_slot,
            1,
        );
        if err != 0 { return None; }
        let err = seL4_Frame_Map(tls_frame_slot, pml4_slot, TLS_BASE, CapRights::ALL.bits(), 0);
        if err != 0 { return None; }
        // Initialize TLS
        unsafe {
            let dest = TLS_BASE as *mut usize;
            for i in 0..(PAGE_SIZE / 8) {
                dest.add(i).write_volatile(0);
            }
            dest.add(0).write_volatile(TLS_BASE);      // tcb pointer
            dest.add(2).write_volatile(TLS_BASE);      // self pointer
            dest.add(5).write_volatile(0x4141414141414141); // stack_guard
            dest.add(6).write_volatile(0x4242424242424242); // pointer_guard
        }
        seL4_DebugPutString("[loader] TLS at 0x");
        print_hex(TLS_BASE);
        seL4_DebugPutChar(b'\n');
    }

    seL4_DebugPutString("[loader] Child VSpace created, PML4 at slot ");
    print_hex(pml4_slot);
    seL4_DebugPutChar(b'\n');

    Some(pml4_slot)
}

/// Holds the slots needed to manage a child task.
pub struct ChildTask {
    pub tcb: usize,
    pub fault_ep: usize,
    pub cnode: usize,
    pub pml4: usize,
    pub ipc_frame: usize,
}

/// Step 4: Create TCB, fault endpoint, and configure the child task.
///
/// Configures the TCB with:
/// - `fault_ep` as the fault handler endpoint
/// - `cnode` as the CSpace root (with radix CHILD_CNODE_BITS)
/// - `pml4` as the VSpace root
/// - IPC buffer at `IPC_BUF_VADDR` backed by a dedicated frame
pub fn create_child_tcb(
    bi: &BootInfo,
    cnode: usize,
    pml4: usize,
) -> Option<ChildTask> {
    // Allocate slots
    let tcb_slot = { OBJ_ALLOCATOR.lock().alloc()? };
    let fault_ep_slot = { OBJ_ALLOCATOR.lock().alloc()? };
    let ipc_frame_slot = { OBJ_ALLOCATOR.lock().alloc()? };

    // Create TCB
    let (tcb_ut, _) = bi.find_free_untyped(ObjectType::TCB.size_bits() as u8)?;
    let err = seL4_Untyped_Retype(
        tcb_ut, ObjectType::TCB as usize,
        ObjectType::TCB.size_bits(), init_slots::CNODE, init_slots::CNODE,
        64, tcb_slot, 1,
    );
    if err != 0 {
        seL4_DebugPutString("[loader] TCB retype failed err=");
        print_hex(err as usize);
        seL4_DebugPutChar(b'\n');
        return None;
    }

    // Create fault endpoint
    let (ep_ut, _) = bi.find_free_untyped(ObjectType::Endpoint.size_bits() as u8)?;
    let err = seL4_Untyped_Retype(
        ep_ut, ObjectType::Endpoint as usize,
        ObjectType::Endpoint.size_bits(), init_slots::CNODE, init_slots::CNODE,
        64, fault_ep_slot, 1,
    );
    if err != 0 {
        seL4_DebugPutString("[loader] Fault EP retype failed err=");
        print_hex(err as usize);
        seL4_DebugPutChar(b'\n');
        return None;
    }

    // Ensure a PageTable exists for the IPC buffer region (0xF00000-0xF1FFFF).
    // The root VSpace only has PTs for the root task's own address range;
    // addresses outside that range (like 0xF11000) need explicit PT creation.
    {
        let pt_slot = { OBJ_ALLOCATOR.lock().alloc()? };
        let (pt_ut, _) = bi.find_free_untyped(12)?;
        let err = seL4_Untyped_Retype(pt_ut, ObjectType::PageTable as usize, 12,
            init_slots::CNODE, init_slots::CNODE, 64, pt_slot, 1);
        if err != 0 { return None; }
        let err = seL4_PageTable_Map(pt_slot, pml4, 0xF00000, 0);
        if err != 0 {
            seL4_DebugPutString("[loader] PT map for IPC region failed err=");
            print_hex(err as usize);
            seL4_DebugPutChar(b'\n');
            return None;
        }
    }

    // Also ensure PT for the stack region (0xF00000, same 2MB region covers both)
    // Stack is at 0xF10000, IPC at 0xF11000 — both in the same 2MB PT.

    // Create IPC buffer frame and map into VSpace
    let (ipc_ut, _) = bi.find_free_untyped(12)?;
    let err = seL4_Untyped_Retype(
        ipc_ut, ObjectType::Frame4K as usize, 12,
        init_slots::CNODE, init_slots::CNODE, 64, ipc_frame_slot, 1,
    );
    if err != 0 {
        seL4_DebugPutString("[loader] IPC frame retype failed err=");
        print_hex(err as usize);
        seL4_DebugPutChar(b'\n');
        return None;
    }
    seL4_DebugPutString("[loader] IPC frame slot=");
    print_hex(ipc_frame_slot);
    seL4_DebugPutString(" vspace=");
    print_hex(pml4);
    seL4_DebugPutString(" vaddr=0x");
    print_hex(IPC_BUF_VADDR);
    seL4_DebugPutChar(b'\n');
    let err = seL4_Frame_Map(
        ipc_frame_slot, pml4, IPC_BUF_VADDR,
        CapRights::ALL.bits(), 0,
    );
    if err != 0 {
        seL4_DebugPutString("[loader] IPC frame map failed err=");
        print_hex(err as usize);
        seL4_DebugPutChar(b'\n');
        return None;
    }
    // Initialize IPC buffer content
    unsafe {
        let ipc = &mut *(IPC_BUF_VADDR as *mut IpcBuffer);
        *ipc = IpcBuffer::new();
        ipc.set_receive_slot(cnode, 0, 64);
    }

    // Create stack pages at 0xF10000 (same 2MB PT as IPC buffer)
    for i in 0..4 {
        let stack_slot = { OBJ_ALLOCATOR.lock().alloc()? };
        let (st_ut, _) = bi.find_free_untyped(12)?;
        let err = seL4_Untyped_Retype(st_ut, ObjectType::Frame4K as usize, 12,
            init_slots::CNODE, init_slots::CNODE, 64, stack_slot, 1);
        if err != 0 { return None; }
        let stack_vaddr = STACK_VADDR + i * PAGE_SIZE;
        let err = seL4_Frame_Map(stack_slot, pml4, stack_vaddr, CapRights::ALL.bits(), 0);
        if err != 0 { return None; }
    }

    // Configure TCB
    let err = seL4_TCB_Configure(
        tcb_slot, fault_ep_slot,
        cnode, 0,
        pml4,
        IPC_BUF_VADDR, ipc_frame_slot,
    );
    if err != 0 {
        seL4_DebugPutString("[loader] TCB configure failed err=");
        print_hex(err as usize);
        seL4_DebugPutChar(b'\n');
        return None;
    }
    if err != 0 {
        seL4_DebugPutString("[loader] TCB configure failed err=");
        print_hex(err as usize);
        seL4_DebugPutChar(b'\n');
        return None;
    }
    let err = seL4_TCB_SetSchedParams(tcb_slot, init_slots::TCB, 255, 255);
    if err != 0 {
        seL4_DebugPutString("[loader] SetSchedParams failed err=");
        print_hex(err as usize);
        seL4_DebugPutChar(b'\n');
        return None;
    }

    seL4_DebugPutString("[loader] TCB at ");
    print_hex(tcb_slot);
    seL4_DebugPutString(" fault_ep=");
    print_hex(fault_ep_slot);
    seL4_DebugPutString(" ipc_frame=");
    print_hex(ipc_frame_slot);
    seL4_DebugPutChar(b'\n');

    Some(ChildTask {
        tcb: tcb_slot,
        fault_ep: fault_ep_slot,
        cnode,
        pml4,
        ipc_frame: ipc_frame_slot,
    })
}

/// Step 5: Start the child task — set up stack, trampoline, registers,
/// and begin execution. After this, the child will start sending faults
/// to the fault endpoint configured in `create_child_tcb`.
pub fn start_child_task(task: &ChildTask, entry: usize, tls_base: usize) {
    // Write stack frame at top of the first stack page
    let stack_top = STACK_VADDR + PAGE_SIZE;
    let str_addr = STACK_VADDR + 512; // place "busybox" string here
    let rsp = stack_top - 64;         // leave room for the frame

    unsafe {
        // Write "busybox\0" string
        let s = str_addr as *mut u8;
        for (i, &b) in b"busybox\0".iter().enumerate() {
            s.add(i).write_volatile(b);
        }
        // Write argc/argv/envp
        let base = rsp as *mut usize;
        base.offset(0).write_volatile(1);           // argc = 1
        base.offset(1).write_volatile(str_addr);    // argv[0] = "busybox"
        base.offset(2).write_volatile(0);           // argv[1] = NULL
        base.offset(3).write_volatile(0);           // envp = NULL
    }

    // Write trampoline at STACK_VADDR: wrfsbase eax; movabs $entry, %rax; jmp *%rax
    unsafe {
        let t = STACK_VADDR as *mut u8;
        // wrfsbase eax  (f3 0f ae d0)
        t.add(0).write_volatile(0xf3);
        t.add(1).write_volatile(0x0f);
        t.add(2).write_volatile(0xae);
        t.add(3).write_volatile(0xd0);
        // movabs $entry, %rax  (48 b8 XX XX XX XX XX XX XX XX)
        t.add(4).write_volatile(0x48);
        t.add(5).write_volatile(0xb8);
        let entry_bytes = entry.to_le_bytes();
        for k in 0..8 {
            t.add(6 + k).write_volatile(entry_bytes[k]);
        }
        // jmp *%rax  (ff e0)
        t.add(14).write_volatile(0xff);
        t.add(15).write_volatile(0xe0);
    }

    let regs: [usize; 18] = [
        STACK_VADDR, // 0: RIP = trampoline
        rsp,         // 1: RSP
        0x202,       // 2: RFLAGS
        tls_base,    // 3: RAX = TLS base for wrfsbase
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    let err = seL4_TCB_WriteRegisters(task.tcb, true, 0, 18, &regs);
    if err != 0 {
        seL4_DebugPutString("[loader] WriteRegisters failed err=");
        print_hex(err as usize);
        seL4_DebugPutChar(b'\n');
        return;
    }

    seL4_DebugPutString("[loader] Child started: entry=0x");
    print_hex(entry);
    seL4_DebugPutString(" stack=0x");
    print_hex(STACK_VADDR);
    seL4_DebugPutString(" tls=0x");
    print_hex(tls_base);
    seL4_DebugPutChar(b'\n');
}

fn print_hex(val: usize) {
    for i in (0..16).rev() {
        let nibble = (val >> (i * 4)) & 0xf;
        let c = if nibble < 10 { b'0' + nibble as u8 } else { b'a' + (nibble - 10) as u8 };
        seL4_DebugPutChar(c);
    }
}
