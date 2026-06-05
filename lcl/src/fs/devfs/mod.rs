//! Device filesystem - ported from kernel-thread
//!
//! Provides /dev/null, /dev/zero, /dev/stdin, /dev/stdout, /dev/stderr

use alloc::string::String;
use alloc::vec::Vec;

/// Device file type
pub enum DevFile {
    Null,
    Zero,
    Stdin,
    Stdout,
    Stderr,
}

/// Device filesystem
pub struct DevFs {
    files: Vec<(String, DevFile)>,
}

impl DevFs {
    pub fn new() -> Self {
        let mut files = Vec::new();
        files.push((String::from("null"), DevFile::Null));
        files.push((String::from("zero"), DevFile::Zero));
        files.push((String::from("stdin"), DevFile::Stdin));
        files.push((String::from("stdout"), DevFile::Stdout));
        files.push((String::from("stderr"), DevFile::Stderr));
        Self { files }
    }

    /// Open a device file
    pub fn open(&self, name: &str) -> Option<&DevFile> {
        self.files.iter().find(|(n, _)| n == name).map(|(_, f)| f)
    }

    /// Read from device
    pub fn read(&self, file: &DevFile, buf: &mut [u8]) -> usize {
        match file {
            DevFile::Null => 0,
            DevFile::Zero => {
                buf.fill(0);
                buf.len()
            }
            DevFile::Stdin => 0,
            _ => 0,
        }
    }

    /// Write to device
    pub fn write(&self, file: &DevFile, _buf: &[u8]) -> usize {
        match file {
            DevFile::Null => _buf.len(),
            DevFile::Stdout | DevFile::Stderr => {
                // Write to seL4 debug output
                for &byte in _buf {
                    sel4_sys::seL4_DebugPutChar(byte);
                }
                _buf.len()
            }
            _ => 0,
        }
    }
}
