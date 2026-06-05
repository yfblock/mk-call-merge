//! Syscall handling module

pub mod exec;
pub mod fs;
pub mod mm;
pub mod signal;
pub mod sys;
pub mod thread;

use alloc::sync::Arc;
use crate::task::Sel4Task;

/// Syscall result type
pub type SysResult = Result<usize, i32>;

/// Linux x86_64 syscall numbers
#[repr(usize)]
#[derive(Debug, Clone, Copy)]
pub enum Sysno {
    Read = 0,
    Write = 1,
    Open = 2,
    Close = 3,
    Stat = 4,
    Fstat = 5,
    Lseek = 8,
    Mmap = 9,
    Mprotect = 10,
    Munmap = 11,
    Brk = 12,
    Ioctl = 16,
    Access = 21,
    Pipe = 22,
    Dup = 32,
    Dup2 = 33,
    Nanosleep = 35,
    Getpid = 39,
    Fork = 57,
    Execve = 59,
    Exit = 60,
    Wait4 = 61,
    Kill = 62,
    Fcntl = 72,
    Mkdir = 83,
    Rmdir = 84,
    Creat = 85,
    Unlink = 87,
    Getcwd = 79,
    Chdir = 80,
    Rename = 82,
    Getdents64 = 217,
    Getuid = 102,
    Getgid = 104,
    Geteuid = 107,
    Getegid = 108,
    Futex = 202,
    ClockGettime = 228,
    ExitGroup = 231,
    Openat = 257,
    Mkdirat = 258,
    Unlinkat = 263,
    Renameat = 264,
    Faccessat = 269,
    Pselect6 = 270,
    Ppoll = 271,
    Readv = 19,
    Writev = 20,
    Pread64 = 17,
    Pwrite64 = 18,
    Set_tid_address = 218,
    Ftruncate = 77,
    Sigprocmask = 14,
    RtSigaction = 13,
    RtSigreturn = 15,
    SchedYield = 24,
    Uname = 63,
    Shmget = 29,
    Shmat = 30,
    Shmctl = 31,
    Gettimeofday = 96,
    Prlimit64 = 302,
    Sendfile = 40,
    Fstatat = 262,
    Utimensat = 280,
    Mount = 165,
    Umount2 = 166,
    Statfs = 137,
}

impl TryFrom<usize> for Sysno {
    type Error = ();

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Read),
            1 => Ok(Self::Write),
            2 => Ok(Self::Open),
            3 => Ok(Self::Close),
            4 => Ok(Self::Stat),
            5 => Ok(Self::Fstat),
            8 => Ok(Self::Lseek),
            9 => Ok(Self::Mmap),
            10 => Ok(Self::Mprotect),
            11 => Ok(Self::Munmap),
            12 => Ok(Self::Brk),
            13 => Ok(Self::RtSigaction),
            14 => Ok(Self::Sigprocmask),
            15 => Ok(Self::RtSigreturn),
            16 => Ok(Self::Ioctl),
            17 => Ok(Self::Pread64),
            18 => Ok(Self::Pwrite64),
            19 => Ok(Self::Readv),
            20 => Ok(Self::Writev),
            21 => Ok(Self::Access),
            22 => Ok(Self::Pipe),
            24 => Ok(Self::SchedYield),
            32 => Ok(Self::Dup),
            33 => Ok(Self::Dup2),
            35 => Ok(Self::Nanosleep),
            39 => Ok(Self::Getpid),
            40 => Ok(Self::Sendfile),
            57 => Ok(Self::Fork),
            59 => Ok(Self::Execve),
            60 => Ok(Self::Exit),
            61 => Ok(Self::Wait4),
            62 => Ok(Self::Kill),
            63 => Ok(Self::Uname),
            72 => Ok(Self::Fcntl),
            77 => Ok(Self::Ftruncate),
            79 => Ok(Self::Getcwd),
            80 => Ok(Self::Chdir),
            82 => Ok(Self::Rename),
            83 => Ok(Self::Mkdir),
            84 => Ok(Self::Rmdir),
            85 => Ok(Self::Creat),
            87 => Ok(Self::Unlink),
            96 => Ok(Self::Gettimeofday),
            98 => Ok(Self::Getuid), // Getrusage -> reuse Getuid for now
            102 => Ok(Self::Getuid),
            104 => Ok(Self::Getgid),
            107 => Ok(Self::Geteuid),
            108 => Ok(Self::Getegid),
            137 => Ok(Self::Statfs),
            165 => Ok(Self::Mount),
            166 => Ok(Self::Umount2),
            202 => Ok(Self::Futex),
            217 => Ok(Self::Getdents64),
            218 => Ok(Self::Set_tid_address),
            228 => Ok(Self::ClockGettime),
            231 => Ok(Self::ExitGroup),
            257 => Ok(Self::Openat),
            258 => Ok(Self::Mkdirat),
            262 => Ok(Self::Fstatat),
            263 => Ok(Self::Unlinkat),
            264 => Ok(Self::Renameat),
            269 => Ok(Self::Faccessat),
            270 => Ok(Self::Pselect6),
            271 => Ok(Self::Ppoll),
            280 => Ok(Self::Utimensat),
            29 => Ok(Self::Shmget),
            30 => Ok(Self::Shmat),
            31 => Ok(Self::Shmctl),
            302 => Ok(Self::Prlimit64),
            _ => Err(()),
        }
    }
}

/// Handle a syscall from a child task
///
/// On x86_64 Linux, syscall number is in RAX, args in RDI, RSI, RDX, R10, R8, R9
/// Returns are placed in RAX
pub async fn handle_syscall(task: &Arc<Sel4Task>, regs: &mut [usize; 20]) -> SysResult {
    // In seL4 UserContext for x86_64:
    // Index 0 = RAX (syscall number / return value)
    // Index 1 = RBX
    // Index 2 = RCX
    // Index 3 = RDX
    // Index 4 = RSI
    // Index 5 = RDI
    // Index 6 = RBP
    // Index 7 = RSP
    // Index 8 = R8
    // Index 9 = R9
    // Index 10 = R10
    // Index 11 = R11
    // Index 12 = R12
    // Index 13 = R13
    // Index 14 = R14
    // Index 15 = RIP
    // Index 16 = RFLAGS

    let syscall_no = regs[0]; // RAX
    let a0 = regs[5]; // RDI
    let a1 = regs[4]; // RSI
    let a2 = regs[3]; // RDX
    let a3 = regs[10]; // R10
    let _a4 = regs[8]; // R8
    let _a5 = regs[9]; // R9

    let sysno = match Sysno::try_from(syscall_no) {
        Ok(s) => s,
        Err(()) => {
            sel4_sys::seL4_DebugPutString("[lcl] Unknown syscall: ");
            // Can't easily print the number without alloc
            sel4_sys::seL4_DebugPutChar(b'\n');
            return Err(38); // ENOSYS
        }
    };

    match sysno {
        // Process management
        Sysno::Exit => thread::sys_exit(task, a0 as u32),
        Sysno::ExitGroup => thread::sys_exit_group(task, a0 as u32),
        Sysno::Getpid => thread::sys_getpid(task),
        Sysno::Execve => exec::sys_execve(task, a0, a1, a2),
        Sysno::Getuid => Ok(0),
        Sysno::Getgid => Ok(0),
        Sysno::Geteuid => Ok(0),
        Sysno::Getegid => Ok(0),
        Sysno::SchedYield => {
            sel4_sys::seL4_Yield();
            Ok(0)
        }
        Sysno::Set_tid_address => thread::sys_set_tid_address(task, a0),

        // System info
        Sysno::Uname => sys::sys_uname(task, a0),

        // Memory management
        Sysno::Brk => mm::sys_brk(task, a0),
        Sysno::Mmap => mm::sys_mmap(task, a0, a1, a2),
        Sysno::Munmap => mm::sys_munmap(task, a0, a1),

        // File I/O - real implementations
        Sysno::Read => fs::sys_read(task, a0, a1, a2),
        Sysno::Write => fs::sys_write(task, a0, a1, a2),
        Sysno::Openat => fs::sys_openat(task, a0 as i32, a1, a2 as u32, a3 as u32),
        Sysno::Close => fs::sys_close(task, a0),
        Sysno::Fstat => fs::sys_fstat(task, a0, a1),
        Sysno::Fstatat => fs::sys_fstatat(task, a0 as i32, a1, a2, a3 as u32),
        Sysno::Lseek => fs::sys_lseek(task, a0, a1 as isize, a2 as i32),
        Sysno::Ioctl => fs::sys_ioctl(task, a0, a1, a2),
        Sysno::Fcntl => fs::sys_fcntl(task, a0, a1, a2),
        Sysno::Readv => fs::sys_readv(task, a0, a1, a2),
        Sysno::Writev => fs::sys_writev(task, a0, a1, a2),
        Sysno::Pread64 => fs::sys_pread64(task, a0, a1, a2, a3 as isize),
        Sysno::Pwrite64 => fs::sys_pwrite64(task, a0, a1, a2, a3 as isize),
        Sysno::Mkdirat => fs::sys_mkdirat(task, a0 as i32, a1, a2 as u32),
        Sysno::Unlinkat => fs::sys_unlinkat(task, a0 as i32, a1, a2 as u32),
        Sysno::Renameat => fs::sys_renameat(task, a0 as i32, a1, a2 as i32, a3),
        Sysno::Ftruncate => fs::sys_ftruncate(task, a0, a1 as isize),
        Sysno::Getdents64 => fs::sys_getdents64(task, a0, a1, a2),
        Sysno::Pipe => fs::sys_pipe2(task, a0, a1 as u32),
        Sysno::Dup => fs::sys_dup(task, a0),
        Sysno::Dup2 => fs::sys_dup3(task, a0, a1, 0),
        Sysno::Sendfile => fs::sys_sendfile(task, a0, a1, a2, a3),
        Sysno::Faccessat => fs::sys_faccessat(task, a0 as i32, a1, a2 as u32, a3 as u32),
        Sysno::Utimensat => fs::sys_utimensat(task, a0 as i32, a1, a2, a3 as u32),
        Sysno::Statfs => fs::sys_statfs(task, a0, a1),
        Sysno::Mount => fs::sys_mount(task, a0, a1, a2, a3, regs[8]),
        Sysno::Umount2 => fs::sys_umount2(task, a0, a1),

        // System info
        Sysno::ClockGettime => sys::sys_clock_gettime(task, a0, a1),
        Sysno::Gettimeofday => sys::sys_gettimeofday(task, a0),

        // Signals
        Sysno::RtSigaction => signal::sys_rt_sigaction(task, a0, a1, a2, a3),
        Sysno::Sigprocmask => signal::sys_sigprocmask(task, a0 as i32, a1, a2),
        Sysno::RtSigreturn => signal::sys_rt_sigreturn(task, regs),
        Sysno::Kill => signal::sys_kill(task, a0, a1),

        // Stubs that return 0
        Sysno::Mprotect => Ok(0),
        Sysno::Nanosleep => Ok(0),
        Sysno::Set_tid_address => thread::sys_set_tid_address(task, a0),

        // Unimplemented
        _ => {
            sel4_sys::seL4_DebugPutString("[lcl] Unimplemented syscall\n");
            Err(38) // ENOSYS
        }
    }
}
