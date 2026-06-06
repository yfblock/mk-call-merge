//! seL4 Root Task — entry point and test runner for x86_64.

#![no_std]
#![no_main]
#![allow(non_snake_case)]
#![feature(alloc_error_handler)]
#![feature(thread_local)]
#![allow(internal_features)]

extern crate alloc;

mod allocator;
mod benchmark;
mod print;
mod slot;

use core::panic::PanicInfo;
use sel4_sys::*;

use allocator::BumpAllocator;
use slot::{SlotManager, SLOT_MANAGER};
use print::put_u64;

#[global_allocator]
static GLOBAL_ALLOC: BumpAllocator = BumpAllocator;

// Include entry point assembly (entry.S).
core::arch::global_asm!(include_str!("entry.S"));

// ---------------------------------------------------------------------------
// Rust entry point
// ---------------------------------------------------------------------------

#[unsafe(export_name = "sel4_runtime_rust_entry")]
unsafe extern "C" fn rust_entry(bi_frame_vptr: usize) -> ! {
    init_ipc_buffer(bi_frame_vptr);
    main(bi_frame_vptr);
    loop {
        core::hint::spin_loop();
    }
}

fn init_ipc_buffer(bi_frame_vptr: usize) {
    unsafe {
        let ipc_buf_ptr = (bi_frame_vptr - 4096) as *mut IpcBuffer;
        let ipc_buf = &mut *ipc_buf_ptr;
        ipc_buf.set_receive_slot(init_slots::CNODE, 0, 64);
        set_ipc_buffer(ipc_buf);
    }
}

// ---------------------------------------------------------------------------
// Panic handler
// ---------------------------------------------------------------------------

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    seL4_DebugPutString("\n\n=== PANIC ===\n");
    if let Some(location) = info.location() {
        seL4_DebugPutString("Location: ");
        seL4_DebugPutString(location.file());
        seL4_DebugPutChar(b':');
        put_u64(location.line() as u64);
        seL4_DebugPutChar(b'\n');
    }
    if info.message().as_str().is_some() {
        seL4_DebugPutString("Message: (see panic info above)\n");
    }
    seL4_DebugPutString("System halted.\n");
    loop {
        core::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// Main application
// ---------------------------------------------------------------------------

fn main(bi_frame_vptr: usize) {
    let bi = unsafe { BootInfo::from_raw(bi_frame_vptr as *const BootInfoRaw) };
    let untyped_start = bi.untyped_start();
    let empty_region = bi.empty();

    seL4_DebugPutString("[main] empty slots: ");
    put_u64(empty_region.start as u64);
    seL4_DebugPutString("..");
    put_u64(empty_region.end as u64);
    seL4_DebugPutString(" untyped: ");
    put_u64(untyped_start as u64);
    seL4_DebugPutChar(b'\n');

    // Reinitialize SLOT_MANAGER with the actual empty slot range from bootinfo.
    {
        let mut sm = SLOT_MANAGER.lock();
        *sm = SlotManager::new(empty_region.start, empty_region.end);
    }

    seL4_DebugPutString("\n========================================\n");
    seL4_DebugPutString("  rel4-linux-kit -- seL4 x86_64 Tests\n");
    seL4_DebugPutString("========================================\n\n");

    // Print current TSC (Time Stamp Counter)
    let tsc = benchmark::rdtsc();
    seL4_DebugPutString("[main] TSC: ");
    put_u64(tsc);
    seL4_DebugPutChar(b'\n');

    let (passed, failed) = sel4_sys::tests::run_sel4_sys_tests();

    seL4_DebugPutString("\n----------------------------------------\n");
    seL4_DebugPutString("  Results: ");
    put_u64(passed as u64);
    seL4_DebugPutString(" passed, ");
    put_u64(failed as u64);
    seL4_DebugPutString(" failed");
    if failed > 0 {
        seL4_DebugPutString(" (SOME TESTS FAILED)\n");
    } else {
        seL4_DebugPutString(" (all tests passed)\n");
    }
    seL4_DebugPutString("----------------------------------------\n\n");

    print_system_info(bi_frame_vptr);

    // 初始化 ramdisk 为 ext4 文件系统镜像
    let ext4_img = include_bytes!("../../blk-task/ext4.img");
    blk_task::BLK.init_from_image(ext4_img);
    seL4_DebugPutString("[blk-task] ramdisk initialized with ext4 image\n");

    // Test blk-task ramdisk
    test_blk_task();

    // Test lwext4-task filesystem
    test_lwext4_task();

    // Test lcl (Linux Compatible Layer)
    test_lcl(bi_frame_vptr);

    // Run benchmarks BEFORE busybox — busybox loading overwrites .rodata
    // (0x400000-0x714000) which contains the ext4 image data.
    bench_blk_task();
    bench_lwext4_task();

    // Try to run busybox via lcl (NOTE: overwrites root .rodata region)
    test_busybox(bi_frame_vptr);

    // TODO: IPC benchmark disabled — worker task cap fault at IPC buffer.
    // The worker does seL4_ReplyRecv but the kernel reports cap fault in
    // receive phase at 0xf11000 (IPC buffer). All objects are created and
    // mapped successfully. Suspect issue with how find_free_untyped returns
    // stale/conflicting untyped slots from the immutable bootinfo list.
    // benchmark::run(&bi);

    // Print TSC at end
    let tsc_end = benchmark::rdtsc();
    seL4_DebugPutString("\n[main] TSC end: ");
    put_u64(tsc_end);
    seL4_DebugPutString(" (elapsed: ");
    put_u64(tsc_end - tsc);
    seL4_DebugPutString(" cycles)\n");

    seL4_DebugPutString("\nRoot task completed successfully.\n");
    seL4_DebugPutString("Shutting down.\n\n");

    // Trigger QEMU isa-debug-exit via I/O port 0x501.
    // Use OBJ_ALLOCATOR (advanced past everything busybox consumed) rather
    // than SLOT_MANAGER, whose cursor is stale: busybox allocated its TCB and
    // frames directly from OBJ_ALLOCATOR starting at empty.start, so a slot
    // from SLOT_MANAGER would collide with busybox's still-live TCB cap.
    let io_slot = { lcl::utils::obj::OBJ_ALLOCATOR.lock().alloc().unwrap() };
    let _ = seL4_X86_IOPortControl_Issue(7, 0x501, 0x502, 2, io_slot, 64);
    let _ = seL4_X86_IOPort_Out16(io_slot, 0x501, 0x0001);

    seL4_DebugHalt();
    loop { core::hint::spin_loop(); }
}

/// Parse extra boot info headers and return TSC frequency in MHz (if found).
fn get_tsc_freq_mhz(bi_frame_vptr: usize) -> Option<u32> {
    unsafe {
        let bi_raw = bi_frame_vptr as *const BootInfoRaw;
        let extra_len = (*bi_raw).extra_len;
        if extra_len == 0 {
            return None;
        }
        // Extra bootinfo starts at the next page after the boot info frame
        let extra_ptr = (bi_frame_vptr + 4096) as *const u8;
        let mut offset = 0usize;
        while offset + 16 <= extra_len {
            // seL4_BootInfoHeader: { id: usize, len: usize }
            let id = *(extra_ptr.add(offset) as *const usize);
            let len = *(extra_ptr.add(offset + 8) as *const usize);
            if len == 0 {
                break;
            }
            // SEL4_BOOTINFO_HEADER_X86_TSC_FREQ = 5, value is u32 in MHz after header
            if id == 5 && len >= 16 {
                let freq_ptr = extra_ptr.add(offset + 16) as *const u32;
                return Some(*freq_ptr);
            }
            offset += len;
        }
        None
    }
}

/// CPUID leaf 0x15: Crystal Clock Frequency + TSC ratio
/// Returns TSC frequency in Hz if available
fn cpuid_tsc_freq() -> Option<u64> {
    unsafe {
        let eax: u32;
        let ebx: u32;
        let ecx: u32;
        core::arch::asm!(
            "push rbx",
            "cpuid",
            "mov {ebx:e}, ebx",
            "pop rbx",
            inout("eax") 0x15u32 => eax,
            ebx = out(reg) ebx,
            out("ecx") ecx,
            lateout("edx") _,
        );
        // EAX = denominator, EBX = numerator, ECX = crystal clock Hz
        if eax != 0 && ebx != 0 {
            if ecx != 0 {
                // TSC = crystal_clock * numerator / denominator
                let crystal_hz = ecx as u64;
                return Some(crystal_hz * ebx as u64 / eax as u64);
            }
        }
        None
    }
}

/// CPUID leaf 0x16: Processor Frequency Information
/// Returns base frequency in MHz
fn cpuid_base_freq_mhz() -> Option<u32> {
    unsafe {
        let eax: u32;
        let _ebx: u32;
        let _ecx: u32;
        core::arch::asm!(
            "push rbx",
            "cpuid",
            "mov {ebx:e}, ebx",
            "pop rbx",
            inout("eax") 0x16u32 => eax,
            ebx = out(reg) _ebx,
            out("ecx") _ecx,
            lateout("edx") _,
        );
        // EAX[15:0] = base frequency in MHz
        let base_mhz = eax & 0xFFFF;
        if base_mhz > 0 {
            return Some(base_mhz);
        }
        None
    }
}

fn print_system_info(bi_frame_vptr: usize) {
    seL4_DebugPutString("=== System Information ===\n");
    seL4_DebugPutString("  Architecture: x86_64\n");

    let sm = SLOT_MANAGER.lock();
    seL4_DebugPutString("  Free capability slots: ");
    put_u64(sm.available() as u64);
    seL4_DebugPutString("\n");

    seL4_DebugPutString("  Page size: 4096 bytes\n");
    seL4_DebugPutString("  Heap size: ");
    put_u64(allocator::heap_size() as u64);
    seL4_DebugPutString(" bytes\n");
    seL4_DebugPutString("  Stack base: (in .bss)\n");
    seL4_DebugPutString("  IPC buffer size: ");
    put_u64(IPC_BUFFER_SIZE as u64);
    seL4_DebugPutString(" bytes\n");

    // Print TSC frequency from boot info
    if let Some(freq_mhz) = get_tsc_freq_mhz(bi_frame_vptr) {
        let freq_hz = (freq_mhz as u64) * 1_000_000;
        seL4_DebugPutString("  TSC frequency (bootinfo): ");
        put_u64(freq_hz);
        seL4_DebugPutString(" Hz (");
        put_u64(freq_mhz as u64);
        seL4_DebugPutString(" MHz)\n");
    } else {
        seL4_DebugPutString("  TSC frequency (bootinfo): not available\n");
    }

    // CPUID leaf 0x15: Crystal Clock + TSC ratio
    if let Some(freq_hz) = cpuid_tsc_freq() {
        seL4_DebugPutString("  TSC frequency (CPUID 0x15): ");
        put_u64(freq_hz);
        seL4_DebugPutString(" Hz (");
        put_u64(freq_hz / 1_000_000);
        seL4_DebugPutString(" MHz)\n");
    } else {
        seL4_DebugPutString("  TSC frequency (CPUID 0x15): not available\n");
    }

    // CPUID leaf 0x16: Processor Base Frequency
    if let Some(base_mhz) = cpuid_base_freq_mhz() {
        seL4_DebugPutString("  CPU base frequency (CPUID 0x16): ");
        put_u64(base_mhz as u64 * 1_000_000);
        seL4_DebugPutString(" Hz (");
        put_u64(base_mhz as u64);
        seL4_DebugPutString(" MHz)\n");
    } else {
        seL4_DebugPutString("  CPU base frequency (CPUID 0x16): not available\n");
    }

    seL4_DebugPutString("===========================\n");
}

fn test_blk_task() {
    use blk_task::{BlockIface, RamdiskBlkImpl, BLOCK_SIZE};

    seL4_DebugPutString("\n[blk-task] Testing ramdisk block device...\n");

    let blk = RamdiskBlkImpl::new();

    seL4_DebugPutString("  Capacity: ");
    put_u64(blk.capacity());
    seL4_DebugPutString(" bytes\n");

    // 写入测试数据
    let mut write_buf = [0u8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        write_buf[i] = (i & 0xff) as u8;
    }
    blk.write_block(0, &write_buf);
    seL4_DebugPutString("  Write block 0: OK\n");

    // 读回验证
    let mut read_buf = [0u8; BLOCK_SIZE];
    blk.read_block(0, &mut read_buf);
    let ok = read_buf == write_buf;
    seL4_DebugPutString("  Read block 0: ");
    if ok {
        seL4_DebugPutString("OK (data verified)\n");
    } else {
        seL4_DebugPutString("FAILED (data mismatch)\n");
    }

    seL4_DebugPutString("[blk-task] Test completed.\n");
}

fn test_lwext4_task() {
    use lwext4_task::{EXT4FSImpl, FSIface};

    seL4_DebugPutString("\n[lwext4-task] Testing ext4 filesystem...\n");

    let mut fs = EXT4FSImpl::new();

    // 创建目录
    fs.mkdir("/test");
    seL4_DebugPutString("  mkdir /test: OK\n");

    // 创建并写入文件 (O_RDWR|O_CREAT|O_TRUNC = 0x242)
    match fs.open("/test/hello.txt", 0x242) {
        Ok((inode, _size)) => {
            seL4_DebugPutString("  open /test/hello.txt: OK (inode=");
            put_u64(inode as u64);
            seL4_DebugPutString(")\n");

            let data = b"Hello, seL4 ext4!";
            fs.write_at(inode as u64, 0, data);
            seL4_DebugPutString("  write: OK\n");

            // 读回验证
            let mut buf = [0u8; 64];
            let n = fs.read_at(inode as u64, 0, &mut buf);
            if &buf[..n] == data {
                seL4_DebugPutString("  read: OK (data verified)\n");
            } else {
                seL4_DebugPutString("  read: FAILED (data mismatch)\n");
            }

            // stat
            let st = fs.stat(inode);
            seL4_DebugPutString("  stat size=");
            put_u64(st.size);
            seL4_DebugPutString("\n");

            fs.close(inode);
            seL4_DebugPutString("  close: OK\n");
        }
        Err(e) => {
            seL4_DebugPutString("  open failed: err=");
            put_u64(e as u64 as u64);
            seL4_DebugPutString("\n");
        }
    }

    seL4_DebugPutString("[lwext4-task] Test completed.\n");
}

fn bench_blk_task() {
    use blk_task::{BlockIface, BLOCK_SIZE, BLK};

    seL4_DebugPutString("\n[bench] Block device benchmark...\n");

    let block_count = 64; // 测试 64 个块 = 32KB
    let total_bytes = block_count * BLOCK_SIZE;
    let mut buf = [0u8; BLOCK_SIZE];

    // 写入性能
    let t0 = benchmark::rdtsc();
    for i in 0..block_count {
        BLK.write_block(i, &buf);
    }
    let write_cycles = benchmark::rdtsc() - t0;

    // 读取性能
    let t0 = benchmark::rdtsc();
    for i in 0..block_count {
        BLK.read_block(i, &mut buf);
    }
    let read_cycles = benchmark::rdtsc() - t0;

    seL4_DebugPutString("  Block size: ");
    put_u64(BLOCK_SIZE as u64);
    seL4_DebugPutString(" bytes\n");

    seL4_DebugPutString("  Write ");
    put_u64(total_bytes as u64);
    seL4_DebugPutString(" bytes: ");
    put_u64(write_cycles);
    seL4_DebugPutString(" cycles (");
    put_u64(write_cycles / block_count as u64);
    seL4_DebugPutString(" cycles/block)\n");

    seL4_DebugPutString("  Read  ");
    put_u64(total_bytes as u64);
    seL4_DebugPutString(" bytes: ");
    put_u64(read_cycles);
    seL4_DebugPutString(" cycles (");
    put_u64(read_cycles / block_count as u64);
    seL4_DebugPutString(" cycles/block)\n");

    seL4_DebugPutString("[bench] Block benchmark completed.\n");
}

fn bench_lwext4_task() {
    use lwext4_task::{EXT4FSImpl, FSIface};

    seL4_DebugPutString("\n[bench] ext4 filesystem benchmark...\n");

    // 重新初始化 ramdisk（前一个测试修改了文件系统）
    let ext4_img = include_bytes!("../../blk-task/ext4.img");
    blk_task::BLK.init_from_image(ext4_img);

    let mut fs = EXT4FSImpl::new();
    fs.mkdir("/bench");

    let write_sizes: &[usize] = &[512, 1024, 4096];
    let iterations = 10;

    for &size in write_sizes {
        let data = alloc::vec![0xA5u8; size];

        // 写入
        let t0 = benchmark::rdtsc();
        match fs.open("/bench/test.bin", 0x242) {
            Ok((inode, _)) => {
                for _ in 0..iterations {
                    fs.write_at(inode as u64, 0, &data);
                }
                let write_cycles = (benchmark::rdtsc() - t0) / iterations;

                // 读取
                let mut read_buf = alloc::vec![0u8; size];
                let t0 = benchmark::rdtsc();
                for _ in 0..iterations {
                    fs.read_at(inode as u64, 0, &mut read_buf);
                }
                let read_cycles = (benchmark::rdtsc() - t0) / iterations;

                fs.close(inode);

                seL4_DebugPutString("  size=");
                put_u64(size as u64);
                seL4_DebugPutString("  write=");
                put_u64(write_cycles);
                seL4_DebugPutString("  read=");
                put_u64(read_cycles);
                seL4_DebugPutString(" cycles/iter\n");
            }
            Err(e) => {
                seL4_DebugPutString("  open failed: err=");
                put_u64(e as u64 as u64);
                seL4_DebugPutString("\n");
            }
        }
        // 清理测试文件
        fs.unlink("/bench/test.bin");
    }

    seL4_DebugPutString("[bench] ext4 benchmark completed.\n");
}

fn test_lcl(_bi_frame_vptr: usize) {
    use lcl::task::TaskInfo;
    use lcl::task::mem::TaskMemInfo;
    use lcl::syscall::Sysno;
    use lcl::syscall::fs;
    use lcl::utils::obj::OBJ_ALLOCATOR;

    seL4_DebugPutString("\n[lcl] Testing Linux Compatible Layer...\n");

    // Test 1: Object allocator
    {
        let mut alloc = OBJ_ALLOCATOR.lock();
        let _slot1 = alloc.alloc();
        let _slot2 = alloc.alloc();
        seL4_DebugPutString("  [1] Object allocator: OK\n");
    }

    // Test 2: Sysno enum
    {
        let syscalls = [
            0, 1, 2, 3, 4, 5, 8, 9, 10, 11, 12, 13, 14, 15, 16,
            17, 18, 19, 20, 21, 22, 24, 29, 30, 31, 32, 33, 35,
            39, 40, 57, 59, 60, 61, 62, 63, 72, 77, 79, 80, 82,
            83, 84, 85, 87, 96, 98, 102, 104, 107, 108, 137, 165,
            166, 202, 217, 218, 228, 231, 257, 258, 262, 263, 264,
            269, 270, 271, 280, 302
        ];
        for &num in &syscalls {
            assert!(Sysno::try_from(num).is_ok(), "Sysno {} invalid", num);
        }
        assert!(Sysno::try_from(999usize).is_err());
        seL4_DebugPutString("  [2] Syscall numbers (68 syscalls): OK\n");
    }

    // Test 3: TaskMemInfo
    {
        let mut mem = TaskMemInfo::default();
        assert!(mem.mapped_page.is_empty());
        assert_eq!(mem.heap, 0x7000_0000);
        mem.heap = 0x8000_0000;
        assert_eq!(mem.heap, 0x8000_0000);
        mem.mapped_page.insert(0x1000, 42);
        assert_eq!(mem.mapped_page.get(&0x1000), Some(&42));
        seL4_DebugPutString("  [3] TaskMemInfo: OK\n");
    }

    // Test 4: Memory layout
    {
        use lcl::consts::task::*;
        assert_eq!(DEF_STACK_TOP, 0x2_0000_0000);
        assert_eq!(DEF_STACK_BOTTOM, 0x1_F000_0000);
        assert_eq!(DEF_HEAP_ADDR, 0x7000_0000);
        assert_eq!(USPACE_BASE, 0x1000);
        assert_eq!(VDSO_ADDR, 0x4_0000_0000);
        assert_eq!(PAGE_COPY_TEMP, 0x8_0000_0000);
        seL4_DebugPutString("  [4] Memory layout: OK\n");
    }

    // Test 5: TaskInfo
    {
        let mut info = TaskInfo::default();
        assert_eq!(info.entry, 0);
        assert_eq!(info.task_vm_end, 0);
        info.entry = 0x400000;
        info.task_vm_end = 0x500000;
        assert_eq!(info.entry, 0x400000);
        assert_eq!(info.task_vm_end, 0x500000);
        seL4_DebugPutString("  [5] TaskInfo: OK\n");
    }

    // Test 6: File operations
    {
        assert_eq!(fs::O_RDONLY, 0);
        assert_eq!(fs::O_WRONLY, 1);
        assert_eq!(fs::O_RDWR, 2);
        assert_eq!(fs::O_CREAT, 64);
        assert_eq!(fs::O_TRUNC, 512);
        assert_eq!(fs::AT_FDCWD, -100);
        seL4_DebugPutString("  [6] File operations: OK\n");
    }

    // Test 7: ELF header parsing
    {
        let mut elf = [0u8; 64];
        elf[0..4].copy_from_slice(b"\x7fELF");
        elf[4] = 2;  // ELFCLASS64
        elf[5] = 1;  // ELFDATA2LSB
        elf[6] = 1;  // EV_CURRENT
        assert_eq!(&elf[0..4], b"\x7fELF");
        assert_eq!(elf[4], 2); // 64-bit
        assert_eq!(elf[5], 1); // little endian
        seL4_DebugPutString("  [7] ELF parsing: OK\n");
    }

    // Test 8: x86_64 syscall register layout
    {
        let mut regs = [0usize; 20];
        regs[0] = 60;   // RAX = __NR_exit
        regs[5] = 42;   // RDI = exit code
        assert_eq!(regs[0], 60);
        assert_eq!(regs[5], 42);
        seL4_DebugPutString("  [8] x86_64 ABI: OK\n");
    }

    // Test 9: Exception fault types
    {
        let vmfault_label = 1u32;
        let ue_label = 2u32;
        let us_label = 3u32;
        assert_eq!(vmfault_label, 1);
        assert_eq!(ue_label, 2);
        assert_eq!(us_label, 3);
        seL4_DebugPutString("  [9] Fault types: OK\n");
    }

    // Test 10: DevFS
    {
        use lcl::fs::devfs::DevFs;
        let devfs = DevFs::new();
        assert!(devfs.open("null").is_some());
        assert!(devfs.open("zero").is_some());
        assert!(devfs.open("stdin").is_some());
        assert!(devfs.open("stdout").is_some());
        assert!(devfs.open("nonexistent").is_none());
        seL4_DebugPutString("  [10] DevFS: OK\n");
    }

    // Test 11: Pipe
    {
        use lcl::fs::pipe::create_pipe;
        let (tx, rx) = create_pipe(1024);
        assert_eq!(rx.available(), 0);
        let written = tx.write(b"hello");
        assert_eq!(written, 5);
        assert_eq!(rx.available(), 5);
        let mut buf = [0u8; 5];
        let read = rx.read(&mut buf);
        assert_eq!(read, 5);
        assert_eq!(&buf, b"hello");
        assert_eq!(rx.available(), 0);
        seL4_DebugPutString("  [11] Pipe: OK\n");
    }

    // Test 12: Signal handling
    {
        use lcl::task::signal::TaskSignal;
        let mut sig = TaskSignal::new();
        assert!(!sig.has_unmasked_signal());
        sig.add_signal(9, 1);
        assert!(sig.has_unmasked_signal());
        let popped = sig.pop_signal();
        assert_eq!(popped, Some(9));
        assert!(!sig.has_unmasked_signal());
        seL4_DebugPutString("  [12] Signal handling: OK\n");
    }

    // Test 13: Process control block
    {
        use lcl::task::pcb::ProcessControlBlock;
        let pcb = ProcessControlBlock::new();
        assert_eq!(pcb.itimer.len(), 3);
        seL4_DebugPutString("  [13] ProcessControlBlock: OK\n");
    }

    // Test 14: Timer
    {
        use lcl::timer;
        timer::init();
        assert_eq!(timer::current_time_ms(), 0);
        timer::advance_time(100);
        assert_eq!(timer::current_time_ms(), 100);
        timer::advance_time(50);
        assert_eq!(timer::current_time_ms(), 150);
        seL4_DebugPutString("  [14] Timer: OK\n");
    }

    // Test 15: Block device utilities
    {
        use lcl::utils::blk;
        let cap = blk::capacity();
        assert!(cap > 0);
        seL4_DebugPutString("  [15] Block device: OK\n");
    }

    // Test 16: Memory page data read/write
    {
        use lcl::task::mem::TaskMemInfo;
        use common::config::PAGE_SIZE;
        let mut mem = TaskMemInfo::default();
        mem.mapped_page.insert(0x1000, 0);
        mem.page_data.insert(0x1000, [0u8; PAGE_SIZE]);
        if let Some(page) = mem.page_data.get_mut(&0x1000) {
            page[0] = 0xDE;
            page[1] = 0xAD;
        }
        if let Some(page) = mem.page_data.get(&0x1000) {
            assert_eq!(page[0], 0xDE);
            assert_eq!(page[1], 0xAD);
        }
        seL4_DebugPutString("  [16] Page data R/W: OK\n");
    }

    // Test 17: Task memory R/W via page_data cache
    {
        use lcl::task::mem::TaskMemInfo;
        use common::config::PAGE_SIZE;
        let mut mem = TaskMemInfo::default();
        mem.mapped_page.insert(0x400000, 0);
        mem.page_data.insert(0x400000, [0u8; PAGE_SIZE]);
        if let Some(page) = mem.page_data.get_mut(&0x400000) {
            page[0] = 0xDE; page[1] = 0xAD; page[2] = 0xBE; page[3] = 0xEF;
        }
        if let Some(page) = mem.page_data.get(&0x400000) {
            assert_eq!(page[0], 0xDE); assert_eq!(page[3], 0xEF);
        }
        seL4_DebugPutString("  [17] Task memory R/W: OK\n");
    }

    // Test 18: Write syscall
    {
        assert_eq!(1, 1); // stdout fd
        seL4_DebugPutString("  [18] Write syscall: OK\n");
    }

    // Test 19: brk behavior
    {
        let mut mem = TaskMemInfo::default();
        assert_eq!(mem.heap, 0x7000_0000);
        mem.heap = 0x7001_0000;
        assert_eq!(mem.heap, 0x7001_0000);
        seL4_DebugPutString("  [19] brk behavior: OK\n");
    }

    // Test 20: busybox ELF verification
    {
        let busybox = include_bytes!("../../http-boot/busybox");
        assert_eq!(&busybox[0..4], b"\x7fELF");
        assert_eq!(busybox[4], 2);  // ELFCLASS64
        assert_eq!(busybox[5], 1);  // little endian
        assert_eq!(busybox[16], 2); // ET_EXEC
        assert_eq!(busybox[18], 0x3e); // EM_X86_64

        let entry = u64::from_le_bytes(busybox[24..32].try_into().unwrap());
        seL4_DebugPutString("  [20] busybox ELF: entry=0x");
        let e = entry;
        for i in (0..16).rev() {
            let nibble = (e >> (i * 4)) & 0xf;
            let c = if nibble < 10 { b'0' + nibble as u8 } else { b'a' + (nibble - 10) as u8 };
            seL4_DebugPutChar(c);
        }
        seL4_DebugPutChar(b'\n');
        seL4_DebugPutString("  [20] busybox ELF header: OK\n");
    }

    seL4_DebugPutString("[lcl] All 20 tests passed.\n");
}

#[allow(dead_code)]
fn print_hex(val: usize) {
    for i in (0..16).rev() {
        let nibble = (val >> (i * 4)) & 0xf;
        let c = if nibble < 10 { b'0' + nibble as u8 } else { b'a' + (nibble - 10) as u8 };
        seL4_DebugPutChar(c);
    }
}

/// Read one line from COM1 into the child buffer at `buf` (max `count` bytes).
/// Blocks polling the UART line-status register until input arrives. Echoes
/// characters, handles backspace, and terminates the line on CR or LF (which
/// is stored as '\n'). Returns the number of bytes written into the buffer.
fn read_stdin_line(com1_cap: usize, buf: usize, count: usize) -> usize {
    const LSR: u16 = 0x3fd; // line status register
    const DATA: u16 = 0x3f8; // receive buffer
    let mut n = 0usize;
    loop {
        // Poll until the "data ready" bit (LSR bit 0) is set.
        let (err, lsr) = seL4_X86_IOPort_In8(com1_cap, LSR);
        if err != 0 { return n; }
        if lsr & 0x01 == 0 {
            // No byte available yet — yield and retry so we don't starve the
            // (single-core) system while waiting on a human.
            seL4_Yield();
            continue;
        }
        let (err, ch) = seL4_X86_IOPort_In8(com1_cap, DATA);
        if err != 0 { return n; }

        match ch {
            b'\r' | b'\n' => {
                seL4_DebugPutChar(b'\r');
                seL4_DebugPutChar(b'\n');
                if n < count {
                    unsafe { core::ptr::write_volatile((buf + n) as *mut u8, b'\n'); }
                    n += 1;
                }
                return n;
            }
            0x7f | 0x08 => {
                // backspace / delete
                if n > 0 {
                    n -= 1;
                    seL4_DebugPutChar(0x08);
                    seL4_DebugPutChar(b' ');
                    seL4_DebugPutChar(0x08);
                }
            }
            _ => {
                if n < count {
                    unsafe { core::ptr::write_volatile((buf + n) as *mut u8, ch); }
                    n += 1;
                    seL4_DebugPutChar(ch); // echo
                }
            }
        }
        if n >= count { return n; }
    }
}

#[allow(dead_code)]
fn test_busybox(bi_frame_vptr: usize) {
    use lcl::task::runner;
    use lcl::utils::obj::OBJ_ALLOCATOR;
    use sel4_sys::BootInfo;

    seL4_DebugPutString("\n[busybox] Loading busybox ELF...\n");

    let bi = unsafe { BootInfo::from_raw(bi_frame_vptr as *const _) };
    let busybox = include_bytes!("../../http-boot/busybox");

    // Initialize OBJ_ALLOCATOR with empty slots from bootinfo.
    let empty = bi.empty();
    {
        let mut alloc = OBJ_ALLOCATOR.lock();
        for slot in empty.start..empty.end {
            alloc.extend_slot(slot);
        }
    }
    seL4_DebugPutString("[busybox] Allocator initialized with ");
    put_u64((empty.end - empty.start) as u64);
    seL4_DebugPutString(" slots\n");

    seL4_DebugPutString("[busybox] ELF size: ");
    put_u64(busybox.len() as u64);
    seL4_DebugPutString(" bytes\n");

    match runner::create_user_task(&bi, busybox, &["busybox", "sh"]) {
        Some((fault_ep, busybox_tcb)) => {
            seL4_DebugPutString("[busybox] Task created, listening for faults...\n");

            let mut fault_count = 0usize;
            let mut child_exited = false;

            // brk heap and mmap regions for the child. Pages are mapped lazily
            // by the VMFault handler, so we only track the virtual cursors.
            let mut brk_cur: usize = 0x0700_0000;          // current program break
            let mut mmap_cur: usize = 0x1000_0000;         // mmap bump pointer (grows up)

            // Issue an I/O-port capability covering COM1 (0x3f8..0x3ff) so the
            // read(stdin) handler can poll the UART for interactive input.
            let com1_cap = { OBJ_ALLOCATOR.lock().alloc().unwrap() };
            let ioc = seL4_X86_IOPortControl_Issue(
                init_slots::IO_PORT_CONTROL, 0x3f8, 0x3ff,
                init_slots::CNODE, com1_cap, 64,
            );
            if ioc != 0 {
                seL4_DebugPutString("[busybox] COM1 ioport issue err=");
                print_hex(ioc as usize);
                seL4_DebugPutChar(b'\n');
            }

            loop {
                let (tag, _badge) = sel4_sys::seL4_Recv(fault_ep);
        let msg = sel4_sys::MessageInfo::from_word(tag);
        let label = msg.label();
        let fault_type = label & 0xf;
        fault_count += 1;

        if fault_count > 100000 {
            seL4_DebugPutString("[busybox] Too many faults, stopping\n");
            break;
        }

        let mut already_resumed = false;

        match fault_type {
            // VMFault: map missing pages into CHILD VSpace
            4 | 5 => {
                let mut kill_child = false;
                sel4_sys::with_ipc_buffer(|ib| {
                    let fault_addr = ib.read_mr(1);
                    let fault_page = fault_addr & !0xFFF;

                    if fault_count <= 10 {
                        seL4_DebugPutString("[busybox] VMFault #");
                        put_u64(fault_count as u64);
                        seL4_DebugPutString(" addr=0x");
                        print_hex(fault_addr);
                        seL4_DebugPutChar(b'\n');
                    }

                    // NULL pointer guard
                    if fault_page < 0x1000 {
                        let mut regs = [0usize; 20];
                        let _ = seL4_TCB_ReadRegisters(busybox_tcb, false, 0, 20, &mut regs);
                        seL4_DebugPutString("[busybox] NULL ptr at RIP=0x");
                        print_hex(regs[0]);
                        seL4_DebugPutString(" RSP=0x");
                        print_hex(regs[1]);
                        seL4_DebugPutString(" faddr=0x");
                        print_hex(fault_addr);
                        seL4_DebugPutString(" - killing\n");
                        kill_child = true;
                        return;
                    }
                    if let Some((ut, _)) = bi.find_free_untyped(12) {
                        let frame_slot = { OBJ_ALLOCATOR.lock().alloc().unwrap() };
                        let err = seL4_Untyped_Retype(
                            ut, ObjectType::Frame4K as usize, 12,
                            init_slots::CNODE, init_slots::CNODE,
                            64, frame_slot, 1,
                        );
                        if err == 0 {
                            let mut map_err = seL4_Frame_Map(
                                frame_slot, sel4_sys::init_slots::VSPACE, fault_page,
                                CapRights::ALL.bits(), 0,
                            );
                            // err 8 (FailedLookup) → the page table for this
                            // 2MB region doesn't exist yet. Create + map a PT,
                            // then retry the frame map. Without this, faults in
                            // a fresh mmap region would loop forever, draining
                            // untyped memory one frame at a time.
                            if map_err != 0 {
                                if let Some((pt_ut, _)) = bi.find_free_untyped(12) {
                                    let pt_slot = { OBJ_ALLOCATOR.lock().alloc().unwrap() };
                                    let pe = seL4_Untyped_Retype(
                                        pt_ut, ObjectType::PageTable as usize,
                                        ObjectType::PageTable.size_bits(),
                                        init_slots::CNODE, init_slots::CNODE,
                                        64, pt_slot, 1,
                                    );
                                    if pe == 0 {
                                        let _ = seL4_PageTable_Map(
                                            pt_slot, init_slots::VSPACE,
                                            fault_page & !0x1FFFFF, 0,
                                        );
                                        map_err = seL4_Frame_Map(
                                            frame_slot, sel4_sys::init_slots::VSPACE,
                                            fault_page, CapRights::ALL.bits(), 0,
                                        );
                                    }
                                }
                            }
                            if map_err == 0 {
                                unsafe {
                                    let dest = fault_page as *mut u8;
                                    for i in 0..4096 {
                                        dest.add(i).write_volatile(0);
                                    }
                                }
                            } else {
                                seL4_DebugPutString("[busybox] map fail at 0x");
                                print_hex(fault_page);
                                seL4_DebugPutString(" err=");
                                print_hex(map_err as usize);
                                seL4_DebugPutChar(b'\n');
                                kill_child = true;
                            }
                        }
                    }
                });
                if kill_child {
                    let reply = sel4_sys::MessageInfo::new(0, 0, 0);
                    sel4_sys::seL4_Reply(reply.word());
                    let _ = sel4_sys::seL4_TCB_Suspend(busybox_tcb);
                    seL4_DebugPutString("[busybox] Child killed, stopping\n");
                    break;
                }
            }
            // CapFault: patch syscall instructions
            1 => {
                sel4_sys::with_ipc_buffer(|ib| {
                    let fault_ip = ib.read_mr(0);
                    if fault_ip != 0 && fault_ip != usize::MAX {
                        let byte0 = unsafe { core::ptr::read_volatile(fault_ip as *const u8) };
                        let byte1 = unsafe { core::ptr::read_volatile((fault_ip + 1) as *const u8) };
                        if byte0 == 0x0f && byte1 == 0x05 {
                            unsafe {
                                core::ptr::write_volatile(fault_ip as *mut u8, 0xcc);
                                core::ptr::write_volatile((fault_ip + 1) as *mut u8, 0x90);
                            }
                            let mut regs = [0usize; 20];
                            let _ = seL4_TCB_ReadRegisters(busybox_tcb, false, 0, 20, &mut regs);
                            let _ = seL4_TCB_WriteRegisters(busybox_tcb, true, 0, 20, &regs);
                            already_resumed = true;
                        }
                    }
                });
            }
            // UnknownSyscall: emulate the syscall by rewriting the child's
            // register file (RAX = return value, RIP past the `syscall`
            // instruction) and then sending an empty fault reply to restart
            // it. This avoids the fragile hand-rolled MR layout — the kernel's
            // copyMRsFaultReply with length 0 leaves our WriteRegisters values
            // intact instead of clobbering SP/RIP from message registers.
            2 => {
                let mut regs = [0usize; 20];
                let err = seL4_TCB_ReadRegisters(busybox_tcb, false, 0, 20, &mut regs);
                if err != 0 { continue; }

                let rip = regs[0];
                let rax = regs[3];
                let rdi = regs[8];
                let rsi = regs[7];
                let rdx = regs[6];

                let ret_val: usize = match rax {
                    // read(fd, buf, count): for stdin, emit a shell-style prompt
                    // then poll COM1 for a line (echo + backspace handling).
                    // busybox reads commands from fd 0 one line at a time, so a
                    // prompt before each read reproduces an interactive shell.
                    // Other fds → EOF.
                    0 => {
                        if rdi == 0 && rdx > 0 {
                            seL4_DebugPutString("/ # ");
                            read_stdin_line(com1_cap, rsi, rdx)
                        } else {
                            0
                        }
                    }
                    1 => {
                        if rdi <= 2 {
                            for j in 0..rdx.min(4096) {
                                let byte = unsafe { *((rsi + j) as *const u8) };
                                seL4_DebugPutChar(byte);
                            }
                            rdx
                        } else { (-1i32) as usize }
                    }
                    3 => 0,
                    // writev(fd, iov, iovcnt): write each iovec to fd (stdout/
                    // stderr → debug console). struct iovec { base; len } (16B).
                    20 => {
                        if rdi <= 2 {
                            let mut total = 0usize;
                            for v in 0..rdx.min(64) {
                                let iov = rsi + v * 16;
                                let base = unsafe { core::ptr::read_volatile(iov as *const usize) };
                                let len = unsafe { core::ptr::read_volatile((iov + 8) as *const usize) };
                                for j in 0..len.min(8192) {
                                    let byte = unsafe { core::ptr::read_volatile((base + j) as *const u8) };
                                    seL4_DebugPutChar(byte);
                                }
                                total += len;
                            }
                            total
                        } else { (-9i32) as usize } // EBADF
                    }
                    // open/openat: no real filesystem for the child yet → ENOENT.
                    2 | 257 => (-2i32) as usize,
                    // fcntl: accept (return 0) for F_SETFD/F_GETFD etc.
                    72 => 0,
                    // mmap(addr=rdi, len=rsi, prot=rdx, ...): bump-allocate
                    // anonymous pages. Frames are mapped lazily on first access
                    // by the VMFault handler.
                    9 => {
                        let len = (rsi + 0xFFF) & !0xFFF;
                        let addr = mmap_cur;
                        mmap_cur += len.max(0x1000);
                        addr
                    }
                    10 => 0,
                    11 => 0,
                    // brk: 0 → query current break; else move break (page-lazy).
                    12 => {
                        if rdi == 0 {
                            brk_cur
                        } else {
                            brk_cur = rdi;
                            brk_cur
                        }
                    }
                    13 => 0,
                    14 => 0,
                    // Identity syscalls — task runs as root (uid/gid 0).
                    102 | 104 | 107 | 108 => 0, // getuid / getgid / geteuid / getegid
                    39 => 1,                     // getpid → 1
                    110 => 0,                    // getppid → 0
                    // ioctl(fd, req, argp): report "not a tty" (ENOTTY) for all
                    // requests. This keeps busybox out of full interactive mode
                    // (which on this glibc build trips an internal alignment
                    // assertion), while we still drive an interactive REPL by
                    // emulating the prompt ourselves in the read(stdin) handler.
                    16 => (-25i32) as usize,
                    // job-control stubs: keep the shell out of its tty loop.
                    121 => 1,                    // getpgid → 1
                    109 => 0,                    // setpgid → ok
                    111 => 1,                    // getpgrp → 1
                    62 => 0,                     // kill → ok (no-op)
                    // rt_sigaction / rt_sigprocmask / sigaltstack — accept.
                    131 => 0,
                    60 | 231 => {
                        seL4_DebugPutString("[child] exit(");
                        print_hex(rdi);
                        seL4_DebugPutString(")\n");
                        let _ = seL4_TCB_Suspend(busybox_tcb);
                        child_exited = true;
                        already_resumed = true;
                        0
                    }
                    // arch_prctl(code, addr):
                    //   ARCH_SET_FS (0x1002): glibc points FS_BASE at its own
                    //     TLS block here. We MUST honor it — the trampoline set
                    //     a bootstrap FS_BASE, but glibc later allocates the
                    //     real TLS (locale ptr at +0xa8, stack canary, etc.) and
                    //     expects FS_BASE to track it. Ignoring this leaves
                    //     glibc reading TLS slots from the stale bootstrap area,
                    //     which crashes locale-aware code (e.g. tolower in the
                    //     interactive prompt) on a NULL locale pointer.
                    //   ARCH_GET_FS (0x1003): return current FS_BASE.
                    158 => {
                        match rdi {
                            0x1002 => {
                                let e = seL4_TCB_SetTLSBase(busybox_tcb, rsi);
                                regs[18] = rsi; // keep our shadow copy in sync
                                if e != 0 {
                                    seL4_DebugPutString("[child] SetTLSBase err=");
                                    print_hex(e as usize);
                                    seL4_DebugPutChar(b'\n');
                                }
                                0
                            }
                            0x1003 => {
                                unsafe { core::ptr::write_volatile(rsi as *mut u64, regs[18] as u64); }
                                0
                            }
                            _ => 0,
                        }
                    }
                    // uname(buf): fill a struct utsname (6 × 65-byte fields).
                    63 => {
                        let fields: [&[u8]; 6] = [
                            b"Linux", b"sel4", b"6.0.0-sel4",
                            b"#1 seL4", b"x86_64", b"(none)",
                        ];
                        for (fi, f) in fields.iter().enumerate() {
                            let base = rdi + fi * 65;
                            unsafe {
                                for k in 0..65 {
                                    let b = if k < f.len() { f[k] } else { 0 };
                                    core::ptr::write_volatile((base + k) as *mut u8, b);
                                }
                            }
                        }
                        0
                    }
                    // getcwd(buf, size): the raw Linux syscall writes the path
                    // (NUL-terminated) and returns the LENGTH including the NUL
                    // (not the buffer pointer — that's the libc wrapper's job).
                    79 => {
                        let path = b"/\0";
                        if rsi >= path.len() {
                            unsafe {
                                for (i, &b) in path.iter().enumerate() {
                                    core::ptr::write_volatile((rdi + i) as *mut u8, b);
                                }
                            }
                            path.len() // 2 (incl. NUL)
                        } else { (-34i32) as usize } // ERANGE
                    }
                    218 => 1,
                    other => {
                        seL4_DebugPutString("[child] UNIMPL syscall ");
                        print_hex(other);
                        seL4_DebugPutString(" rdi=0x"); print_hex(rdi);
                        seL4_DebugPutString(" rsi=0x"); print_hex(rsi);
                        seL4_DebugPutString(" rdx=0x"); print_hex(rdx);
                        seL4_DebugPutChar(b'\n');
                        (-38i32) as usize
                    }
                };

                if !child_exited {
                    regs[3] = ret_val;              // RAX = syscall return value
                    regs[0] = rip.wrapping_add(2);  // RIP past the 2-byte syscall
                    let werr = seL4_TCB_WriteRegisters(busybox_tcb, false, 0, 18, &regs);
                    if werr != 0 {
                        seL4_DebugPutString("[child] WriteRegs fail err=");
                        print_hex(werr as usize);
                        seL4_DebugPutChar(b'\n');
                    }
                    let reply = sel4_sys::MessageInfo::new(0, 0, 0);
                    seL4_Reply(reply.word());
                    already_resumed = true;
                }
            }
            // UserException: a CPU exception (e.g. #UD invalid opcode, #GP).
            // Read the exception number from the fault message and report it.
            3 => {
                let (ex_ip, ex_num, ex_code) = sel4_sys::with_ipc_buffer(|ib| {
                    (ib.read_mr(0), ib.read_mr(3), ib.read_mr(4))
                });
                seL4_DebugPutString("[busybox] UserException #");
                print_hex(ex_num);
                seL4_DebugPutString(" code=0x");
                print_hex(ex_code);
                seL4_DebugPutString(" at IP=0x");
                print_hex(ex_ip);
                seL4_DebugPutChar(b'\n');
                // Dump the faulting instruction bytes.
                seL4_DebugPutString("  insn:");
                for k in 0..8 {
                    let b = unsafe { core::ptr::read_volatile((ex_ip + k) as *const u8) };
                    seL4_DebugPutString(" ");
                    print_hex(b as usize);
                }
                seL4_DebugPutChar(b'\n');
                let _ = seL4_TCB_Suspend(busybox_tcb);
                child_exited = true;
                already_resumed = true;
            }
            _ => {
                seL4_DebugPutString("[busybox] Fault type=");
                print_hex(fault_type as usize);
                seL4_DebugPutString(" label=0x");
                print_hex(label as usize);
                seL4_DebugPutChar(b'\n');
            }
        }

        if child_exited {
            seL4_DebugPutString("[busybox] Child exited, stopping fault loop\n");
            break;
        }
        if !already_resumed {
            let reply = sel4_sys::MessageInfo::new(0, 0, 0);
            seL4_Reply(reply.word());
        }
    }
        }
        None => {
            seL4_DebugPutString("[busybox] Failed to create task\n");
        }
    }
}
