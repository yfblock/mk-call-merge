//! Linux Compatible Layer (LCL) - seL4-based Linux syscall emulation
//!
//! This crate provides a Linux-compatible environment on top of seL4,
//! allowing Linux ELF binaries to run as child processes.

#![no_std]
#![allow(unused)]
#![allow(dead_code)]

extern crate alloc;

pub mod consts;
pub mod task;
pub mod syscall;
pub mod fs;
pub mod device;
pub mod exception;
pub mod timer;
pub mod utils;
pub mod child_test;
pub mod arch;
