use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

use uefi::prelude::*;
use uefi::table::boot::{BootServices, MemoryType };

pub struct UefiAlloc<'a> {
    boot_services: &'a Option<&'a BootServices>
}

impl<'a> UefiAlloc<'a> {
    pub fn new(bs: &'a Option<&'a BootServices>) -> Self {
        Self { boot_services: bs }
    }
}

unsafe impl<'a> GlobalAlloc for UefiAlloc<'a> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ty = MemoryType::LOADER_DATA;

        // TODO: add support for other alignments.
        if layout.align() > 8 {
            // Unsupported alignment for allocation, UEFI can only allocate 8-byte aligned addresses
            ptr::null_mut()
        } else {
            match self.boot_services.as_ref() {
                Some(bs) => {
                    let buf = bs.allocate_pool(ty, layout.size())
                            .warning_as_error()
                            .unwrap_or(ptr::null_mut());
                    *(buf as *mut u64) = 0x100000000000000;
                    buf.offset(8)
                    // TODO prepend allocate type
                }
                None => ptr::null_mut()
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        self.boot_services.as_ref().unwrap()
            .free_pool(ptr)
            .warning_as_error()
            .unwrap();
    }
}
