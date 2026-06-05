//! Log implementation for seL4

use log::{Level, Log, Metadata, Record, SetLoggerError};

struct SeL4Logger;

impl Log for SeL4Logger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let msg = record.args();
            // Use seL4 debug output
            sel4_sys::seL4_DebugPutString(alloc::format!("[{}] {}\n", record.level(), msg).as_str());
        }
    }

    fn flush(&self) {}
}

static LOGGER: SeL4Logger = SeL4Logger;

/// Initialize the logger
pub fn init() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(Level::Trace.to_level_filter());
}
