use core::fmt::Write;
use core::cell::UnsafeCell;

use log::{Record, Level, Metadata};
use uart_16550::SerialPort;

pub struct Com1Logger {
    port: UnsafeCell<SerialPort>,
}

unsafe impl Sync for Com1Logger {}

// TODO implement thread safety.. but we first need synchronization primitives.

impl Com1Logger {
    pub fn new(port: SerialPort) -> Self {
        Self { port: UnsafeCell::new(port) }
    }
}

impl log::Log for Com1Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let writer = unsafe { self.port.get().as_mut().unwrap() };
            write!(writer, "{} - {}\n", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}
