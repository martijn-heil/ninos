use uefi::table::boot::{MemoryType, MemoryDescriptor};

use log::{debug, info, warn, error};


mod memory;

pub fn init<'a, I>(descriptors: I) where I: Iterator<Item = &'a MemoryDescriptor> + Clone {
    info!("Memory map:");
    for desc in descriptors.clone() {
        info!("{:?}", desc);
    }
    memory::set_up_paging(descriptors);
}
