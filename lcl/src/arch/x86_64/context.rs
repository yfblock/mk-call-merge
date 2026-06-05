//! x86_64 context management
//!
//! Provides register definitions and context manipulation for x86_64 tasks.
//! Modeled after rel4-linux-kit's sel4-ulib UserContext pattern.

/// seL4 UserContext register indices for x86_64
#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Register {
    Rax = 0,
    Rbx = 1,
    Rcx = 2,
    Rdx = 3,
    Rsi = 4,
    Rdi = 5,
    Rbp = 6,
    Rsp = 7,
    R8 = 8,
    R9 = 9,
    R10 = 10,
    R11 = 11,
    R12 = 12,
    R13 = 13,
    R14 = 14,
    Rip = 15,
    Rflags = 16,
}

impl Register {
    pub fn index(self) -> usize {
        self as usize
    }
}

/// x86_64 UserContext - wraps seL4's UserContext with named register access
#[derive(Debug, Clone, Default)]
pub struct UserContext {
    pub regs: [usize; 20],
}

impl UserContext {
    pub fn new() -> Self {
        Self { regs: [0usize; 20] }
    }

    pub fn new_task(entry: usize, sp: usize) -> Self {
        let mut ctx = Self::new();
        ctx.set_pc(entry);
        ctx.set_sp(sp);
        ctx.set_rflags(0x200);
        ctx
    }

    pub fn get(&self, reg: Register) -> usize {
        self.regs[reg.index()]
    }

    pub fn set(&mut self, reg: Register, val: usize) {
        self.regs[reg.index()] = val;
    }

    pub fn pc(&self) -> usize {
        self.regs[Register::Rip.index()]
    }

    pub fn set_pc(&mut self, val: usize) {
        self.regs[Register::Rip.index()] = val;
    }

    pub fn sp(&self) -> usize {
        self.regs[Register::Rsp.index()]
    }

    pub fn set_sp(&mut self, val: usize) {
        self.regs[Register::Rsp.index()] = val;
    }

    pub fn set_rflags(&mut self, val: usize) {
        self.regs[Register::Rflags.index()] = val;
    }

    pub fn syscall_no(&self) -> usize {
        self.regs[Register::Rax.index()]
    }

    pub fn syscall_arg(&self, n: usize) -> usize {
        match n {
            0 => self.regs[Register::Rdi.index()],
            1 => self.regs[Register::Rsi.index()],
            2 => self.regs[Register::Rdx.index()],
            3 => self.regs[Register::R10.index()],
            4 => self.regs[Register::R8.index()],
            5 => self.regs[Register::R9.index()],
            _ => 0,
        }
    }

    pub fn set_return_value(&mut self, val: usize) {
        self.regs[Register::Rax.index()] = val;
    }

    pub fn advance_pc(&mut self) {
        self.regs[Register::Rip.index()] += 4;
    }

    pub fn as_slice(&self) -> &[usize] {
        &self.regs
    }

    pub fn as_mut_slice(&mut self) -> &mut [usize] {
        &mut self.regs
    }
}

pub const SYSCALL_INSTR: u16 = 0x050f;
pub const TRAP_INSTR: u32 = 0xdeadbeef;
pub const RFLAGS_IF: usize = 0x200;
