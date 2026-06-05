//! Pipe implementation - ported from kernel-thread

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use spin::Mutex;

/// Pipe sender
pub struct PipeSender {
    buffer: Arc<Mutex<VecDeque<u8>>>,
    max_size: usize,
}

/// Pipe receiver
pub struct PipeReceiver {
    buffer: Arc<Mutex<VecDeque<u8>>>,
}

/// Create a pipe pair
pub fn create_pipe(max_size: usize) -> (PipeSender, PipeReceiver) {
    let buffer = Arc::new(Mutex::new(VecDeque::new()));
    (
        PipeSender {
            buffer: buffer.clone(),
            max_size,
        },
        PipeReceiver {
            buffer,
        },
    )
}

impl PipeSender {
    /// Write data to pipe
    pub fn write(&self, data: &[u8]) -> usize {
        let mut buf = self.buffer.lock();
        let available = self.max_size - buf.len();
        let to_write = data.len().min(available);
        for &byte in &data[..to_write] {
            buf.push_back(byte);
        }
        to_write
    }
}

impl PipeReceiver {
    /// Read data from pipe
    pub fn read(&self, buf: &mut [u8]) -> usize {
        let mut pipe_buf = self.buffer.lock();
        let to_read = buf.len().min(pipe_buf.len());
        for byte in buf.iter_mut().take(to_read) {
            *byte = pipe_buf.pop_front().unwrap();
        }
        to_read
    }

    /// Check if pipe has data
    pub fn available(&self) -> usize {
        self.buffer.lock().len()
    }
}
