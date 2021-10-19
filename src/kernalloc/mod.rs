#![feature(alloc_error_handler)]


use core::alloc::{GlobalAlloc, Layout};
use core::ptr::{self, NonNull};

use uefi::prelude::*;
use uefi::table::boot::{BootServices, MemoryType, MemoryDescriptor};

use log::{debug, info, warn, error};

use byte_unit::*;

mod uefi_alloc;
mod stat_alloc;

use uefi_alloc::UefiAlloc;
use stat_alloc::StatAlloc;

pub struct Allocator;

mod BlockType {
    pub const UEFI: u8 = 0x10;
    pub const STAT: u8 = 0x20;
}

static mut emergency_heap: Option<*mut u8> = None;
static mut uefialloc: Option<UefiAlloc> = None;
static mut statalloc: Option<StatAlloc> = None;


pub unsafe fn init(bs: &'static Option<&BootServices>) -> Result<(), ()> {
    let pool_size = n_mib_bytes!(1) as usize;
    let pool = bs.unwrap().allocate_pool(MemoryType::LOADER_DATA, pool_size).unwrap();
    unsafe { emergency_heap = Some(pool.unwrap() as *mut u8); }
    info!("Initializing kernalloc.");
    uefialloc = Some(UefiAlloc::new(bs));
    statalloc = Some(StatAlloc::new(emergency_heap.unwrap(), pool_size));
    Ok(())
}

pub unsafe fn exit_boot_services() {
    info!("Exiting boot services for kernalloc.");
    uefialloc = None;
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mem_ty = MemoryType::LOADER_DATA;
        let size = layout.size();
        let align = layout.align();

        // TODO: add support for other alignments.
        if align > 8 {
            ptr::null_mut()
        } else {
            match uefialloc.as_ref() {
                Some(allocator) => allocator.alloc(layout),
                None => {
                    statalloc.as_ref().unwrap().alloc(layout)
                }
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let ty: u8 = *ptr.offset(-8) & 0xF0; // most-significant half-byte of the 64 bit integer right before the buffer

        match ty {
            BlockType::UEFI => if let Some(allocator) = uefialloc.as_ref() { allocator.dealloc(ptr, layout) }
            _ => {}
        }
    }
}

#[alloc_error_handler]
fn out_of_memory(layout: Layout) -> ! {
    panic!(
        "Ran out of free memory while trying to allocate {:#?}",
        layout
    );
}

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
