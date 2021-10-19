
use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

use uefi::prelude::*;
use uefi::table::boot::{BootServices, MemoryType };

pub struct StatAlloc {
    buf: *mut u8,
    maxsize: usize,
    used: usize,
}

impl StatAlloc {
    pub fn new(buf: *mut u8, maxsize: usize) -> Self {
        assert!(maxsize != 0, "maxsize should be at least 1 or more.");
        Self {
            buf: buf,
            maxsize: maxsize,
            used: 0
        }
    }
}

unsafe impl GlobalAlloc for StatAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // TODO: add support for other alignments.
        let align = layout.align();
        let size = layout.size();
        let mutself = self as *const StatAlloc;
        let mutself = mutself as *mut StatAlloc;
        let mutself = mutself.as_mut().unwrap();

        if !align.is_power_of_two() {
            ptr::null_mut()
        } else {
            let mut last = self.buf.add(self.maxsize);
            let mut cursor = self.buf.add(self.used);
            if cursor > last { return ptr::null_mut(); }
            *cursor = 0x20;
            cursor = cursor.add(8);
            if cursor > last { return ptr::null_mut(); }
            let offset = cursor.align_offset(align);
            if offset == usize::max_value() { return ptr::null_mut(); }
            cursor = cursor.add(offset);

            mutself.used += (cursor as usize) - (self.buf as usize) + size;
            cursor
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        // does nothing, we don't dealloc
    }
}

