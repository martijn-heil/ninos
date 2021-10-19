#![no_std]
#![no_main]

#![feature(abi_efiapi)]
#![feature(panic_info_message)]
#![feature(asm)]
#![feature(alloc_error_handler)]
#![feature(lang_items)]
#![feature(alloc_prelude)]
#![feature(link_llvm_intrinsics)]

extern crate uefi;
//extern crate uefi_services;
extern crate log;
extern crate x86;
extern crate x86_64;
extern crate heapless;
extern crate uart_16550;
extern crate byte_unit;
//extern crate prettytable;

#[macro_use]
extern crate alloc;

use alloc::prelude::v1::*;

use core::slice;

use log::{info, warn, error};

use uefi::prelude::*;
use uefi::table::boot::{MemoryType, MemoryDescriptor};


mod arch;
mod kernlog;
mod kernalloc;




pub const MAX_MEMORY_DESCRIPTOR_COUNT: usize = 200;

static mut BOOT_SERVICES: Option<&BootServices> = None;
static mut RUNTIME_SERVICES: Option<&RuntimeServices> = None;


#[entry]
fn uefi_start(image_handle: uefi::Handle, st: SystemTable<Boot>) -> Status {
    //uefi_services::init(&st).expect_success("Failed to initialize uefi_services");
    unsafe {
        BOOT_SERVICES = Some(st.boot_services());
        RUNTIME_SERVICES = Some(st.runtime_services());
    };

    kernlog::init();
    unsafe { kernalloc::init(&BOOT_SERVICES).unwrap(); };
    let rev = st.uefi_revision();
    info!("Welcome to NinOS on UEFI v{}.{}!", rev.major(), rev.minor());

    let bs = st.boot_services();

    //let emergency_heap = bs.allocate_pool(MemoryType::LOADER_DATA, n_mib_bytes!(1) as usize).unwrap();
    let mut memory_descriptors: Vec<MemoryDescriptor> = Vec::with_capacity(200);


    info!("1");
    let n: usize = 2 * bs.memory_map_size();
    let mmap_buf_ptr = bs.allocate_pool(MemoryType::LOADER_DATA, n).unwrap().unwrap();
    let mut mmap_buf = unsafe { slice::from_raw_parts_mut(mmap_buf_ptr, n) };

    info!("Exiting boot services.. i'm gonna be silent for some time now..");
    let res1 = match st.exit_boot_services(image_handle, &mut mmap_buf) {
        Ok(res) => res,
        Err(_err) => { error!("Could not exit boot services."); panic!(); }
    }.log();
    unsafe { BOOT_SERVICES = None; };

    // We're on our own now.
    // Memory allocator is now no longer available,  so we can't do anything fancy until we get an
    // allocator running.
    //
    // TODO what happens with the stack?

    let st = res1.0;
    for desc in res1.1.map(|it| { let mut modified = it.clone(); modified.virt_start = modified.phys_start; modified }) {
        memory_descriptors.push(desc);
    }
    //memory_descriptors.sort_by_key(|it| it.phys_start); TODO this fails because it allocates
    // memory, see core documentation
    /*for (i, v) in memory_descriptors.enumerate() {
        //unsafe { mmap_[i] = Some(v); }
    }*/
    //let present_memory_descriptors: Vec<MemoryDescriptor> = mmap_.iter().filter_map(|it| it.as_deref())

    //let need_to_fill_in_virt_addrs = true;
    /*if need_to_fill_in_virt_addrs {
        for desc in present_memory_descriptors {
            desc.virt_start = desc.phys_start;
        }
    }*/

    //panic!("whoa!");
    let mmap_iter = memory_descriptors.iter();

    info!("Exiting kernalloc boot services..");
    unsafe { kernalloc::exit_boot_services(); }

    arch::amd64::init(mmap_iter);
    info!("We're still alive, hurray!");


    //info!("Ayy!");
    loop {}
}

fn powerdown() -> ! {
    // If running in QEMU, use the f4 exit port to signal the error and exit
    if cfg!(feature = "qemu") {
        use x86_64::instructions::port::Port;
        let mut port = Port::<u32>::new(0xf4);
        unsafe {
            port.write(42);
        }
    }

    // If the system table is available, use UEFI's standard shutdown mechanism
    if let Some(rs) = unsafe { RUNTIME_SERVICES } {
        use uefi::table::runtime::ResetType;
        rs.reset(ResetType::Shutdown, uefi::Status::ABORTED, None);
    }

    // If we don't have any shutdown mechanism handy, the best we can do is loop
    error!("Could not shut down, please power off the system manually...");

    loop {
        unsafe {
            // Try to at least keep CPU from running at 100%
            asm!("hlt" :::: "volatile");
        }
    }
}

#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo) -> ! {
    if let Some(location) = info.location() {
        error!(
            "Panic in {} at ({}, {}):",
            location.file(),
            location.line(),
            location.column()
        );

        if let Some(message) = info.message() {
            error!("{}", message);
        }
    }

    // Give the user some time to read the message
    if let Some(bs) = unsafe { BOOT_SERVICES } {
        bs.stall(10_000_000);
    } else {
        let mut dummy = 0u64;
        // FIXME: May need different counter values in debug & release builds
        for i in 0..300_000_000 {
            unsafe {
                core::ptr::write_volatile(&mut dummy, i);
            }
        }
    }

    powerdown();
}

#[lang = "eh_personality"]
fn eh_personality() {}

/*
 * 0x(0000)000000000000: no read no write no execute empty
 *
 * 0x(0000)000000001000: userland main code
 *                       .text
 *                       .bss
 *                       heap
 *                       ...
 *                       ...
 *
 *                       dylib2 code
 *                       .text
 *                       .bss
 *
 *                       dylib1 code
 *                       .text
 *                       .bss
 * 0x(0000)800000000000: kernel code (LOADER_CODE)
 *                       kernel .text
 *                       kernel .bss
 *                       kernel heap
 *
 * 0x(0000)840000000000: MMIO & MMIO_PORT_SPACE
 *                       ACPI
 *
 * 0x(0000)880000000000: memory-mapped physical memory
 *                          conventional memory
 *
 */
