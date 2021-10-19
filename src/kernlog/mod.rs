/// Kernel logging
use core::fmt::Write;
use core::cell::UnsafeCell;

use log::{Record, Level, Metadata, LevelFilter};
use uart_16550::SerialPort;

mod com1logger;
use com1logger::Com1Logger;

static mut LOGGER: Option<KernLogger> = None;

pub struct KernLogger {
    com1log: Com1Logger
}

const COM1: u16 = 0x3F8;

//unsafe impl Sync for KernLogger {}

// TODO implement thread safety.. but we first need synchronization primitives.

impl KernLogger {
    pub unsafe fn new() -> Self {
        let mut debug_port = unsafe { SerialPort::new(COM1) };
        debug_port.init();

        Self {
            com1log: Com1Logger::new(debug_port)
        }
    }
}

impl log::Log for KernLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            self.com1log.log(record);
        }
    }

    fn flush(&self) {}
}

pub fn init() {
    unsafe { LOGGER = Some(KernLogger::new()); };

    log::set_logger(unsafe { LOGGER.as_ref().unwrap() })
            .map(|()| log::set_max_level(LevelFilter::Info));
}
