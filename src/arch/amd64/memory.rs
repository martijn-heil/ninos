use uefi::prelude::*;
use uefi::table::boot::{MemoryType, MemoryDescriptor};

use x86_64::{PhysAddr, VirtAddr};

use x86_64::structures::paging::page_table::{PageTable, PageTableEntry, PageTableFlags, PageTableIndex};
use x86_64::structures::paging::frame::{PhysFrame, PhysFrameRange, PhysFrameRangeInclusive};
use x86_64::structures::paging::page::{PageSize as PageSizeT, Size4KiB, Page, PageRangeInclusive};
use x86_64::structures::paging::mapper::{Mapper, OffsetPageTable, MapToError};
use x86_64::structures::paging::UnusedPhysFrame;
use x86_64::structures::paging::FrameAllocator;
use x86_64::registers::control::{Cr3, Cr3Flags};


use log::{debug, info, warn, error};

use byte_unit::*;

use heapless::consts::U64 as HLU64;

use alloc::prelude::v1::*;
use alloc::collections::*;


/*extern {
    #[link_name = "llvm.returnaddress"]
    fn return_address() -> *const u8;
}*/


#[derive(Debug, Clone, Copy)]
enum PageSize {
    Huge(),
    Large(),
    Normal(),
}
impl PageSize {
    fn size(&self) -> u64 {
        match self {
            Self::Huge() => n_gib_bytes!(1) as u64,     // 1 GiB
            Self::Large() => n_mib_bytes!(2) as u64,    // 2 MiB
            Self::Normal() => n_kib_bytes!(4) as u64,   // 1 KiB
        }
    }
}


#[derive(Debug, Clone, Copy)]
struct MyMemoryDescriptor {
    phys_start: PhysAddr,
    virt_start: VirtAddr,
    page_size: PageSize,
    page_count: u64,
}

impl From<&MemoryDescriptor> for MyMemoryDescriptor {
    fn from(des: &MemoryDescriptor) -> Self {
        Self {
            phys_start: PhysAddr::new(des.phys_start),
            virt_start: VirtAddr::new(des.virt_start),
            page_size: PageSize::Normal(),
            page_count: des.page_count,
        }
    }
}

/*struct PhysFrameRangeInclusiveIter<S: x86_64::structures::paging::PageSize> {
    frame_range: PhysFrameRangeInclusive<S>,
    cursor: u64,
}

impl<S: x86_64::structures::paging::PageSize> PhysFrameRangeInclusiveIter<S> {
    const PSIZE: u64 = S::SIZE;

    fn new(frame_range: PhysFrameRangeInclusive<S>) -> Self {
        assert!(frame_range.start.start_address() <= frame_range.end.start_address());
        assert!(frame_range.end.start_address() + Self::PSIZE > frame_range.end.start_address());
        assert!(frame_range.start.start_address() + Self::PSIZE > frame_range.start.start_address());

        Self {
            frame_range: frame_range,
            cursor: 0
        }
    }
}

impl<S: x86_64::structures::paging::PageSize> Iterator for PhysFrameRangeInclusiveIter<S> {
    type Item = PhysFrame<S>;

    fn next(&mut self) -> Option<Self::Item> {
        info!("next! cursor: {}", self.cursor);
        let start = self.frame_range.start;
        let end = self.frame_range.end;

        let last_used_addr = PhysAddr::new(start.start_address().as_u64().checked_add(self.cursor.checked_mul(Self::PSIZE.checked_sub(1).unwrap()).unwrap()).unwrap());
        let last_addr = PhysAddr::new(end.start_address().as_u64().checked_add(end.size().checked_sub(1).unwrap()).unwrap());
        let new_addr = PhysAddr::new(start.start_address().as_u64().checked_add(self.cursor.checked_mul(Self::PSIZE).unwrap()).unwrap());
        if last_used_addr == last_addr { return None }

        let new_item: PhysFrame<S> = PhysFrame::from_start_address(new_addr).unwrap();
        self.cursor = self.cursor.checked_add(1).unwrap();
        Some(new_item)
    }
}*/

/// Calculate overhead
/// Note that this does not include the 512 bytes of the level 4 table (root page table)
fn page_table_usage(page_size: PageSize, pages: u64) -> u64 {
    let level1_tables = (pages + 512-1) / 512;
    let level2_tables = (level1_tables + 512-1) / 512;
    let level3_tables = (level2_tables + 512-1) / 512;
    // There is always only a single level 4 table.

    match page_size {
        PageSize::Normal() => level1_tables + level2_tables + level3_tables,
        PageSize::Large() => level2_tables + level2_tables,
        PageSize::Huge() => level3_tables,
    }
}

// I don't like how much memory and processing power this function wastes.. it's tricky because we
// don't have a heap.
/// Get aligned memory frames using given page size.
// TODO is broken
fn get_frames<'a, I>(conventional_memory: I) -> heapless::Vec<MyMemoryDescriptor, HLU64> where I: Iterator<Item = &'a MemoryDescriptor> + Clone  {
    let align_frame = |it: &MemoryDescriptor, page_size_e: PageSize| {
        let frame_size = it.page_count * 4096;
        if frame_size == 0 { return (None, frame_size); }

        let page_size = page_size_e.size();

        let aligned_phys_start: PhysAddr = PhysAddr::new(it.phys_start).align_up(page_size);
        if aligned_phys_start.as_u64() < it.phys_start { return (None, frame_size); }

        let new_virt_start: VirtAddr = VirtAddr::new(it.virt_start + (aligned_phys_start.as_u64() - it.phys_start));

        let old_last_phys: PhysAddr = PhysAddr::new(it.phys_start + (frame_size - 1));

        if aligned_phys_start >= old_last_phys { return (None, frame_size); }

        let page_count: u64 = ((old_last_phys.as_u64()) - aligned_phys_start.as_u64() + 1) / page_size; // Integer rounding on purpose. TODO think this through
        if page_count == 0 { return (None, frame_size); }

        let new_last_phys: PhysAddr = PhysAddr::new(aligned_phys_start.as_u64() + (page_count * page_size - 1));
        let _new_last_virt: VirtAddr = VirtAddr::new(new_virt_start.as_u64() + (page_count * page_size - 1));

        let waste: u64 = (aligned_phys_start.as_u64() - it.phys_start) + (old_last_phys.as_u64() - new_last_phys.as_u64());


        if page_count > 0 {
            (Some(MyMemoryDescriptor {
                phys_start: aligned_phys_start,
                virt_start: new_virt_start,
                page_size: page_size_e,
                page_count: page_count,
            }), waste)
        } else {
            (None, frame_size)
        }
    };

    let huge_frames = conventional_memory.clone().map(|it| align_frame(it, PageSize::Huge()));
    let huge_page_count: u64 = huge_frames.clone().filter_map(|it| it.0).map(|it| it.page_count).sum();
    let huge_alignment_waste = huge_frames.clone().map(|it| it.1).sum::<u64>();
    let huge_wasted: u64 = huge_alignment_waste + page_table_usage(PageSize::Huge(), huge_page_count) * PageSize::Normal().size();
    let _huge_usable: u64 = huge_page_count * PageSize::Huge().size();

    let large_frames = conventional_memory.clone().map(|it| align_frame(it, PageSize::Large()));
    let large_page_count: u64 = large_frames.clone().filter_map(|it| it.0).map(|it| it.page_count).sum();
    let large_alignment_waste = large_frames.clone().map(|it| it.1).sum::<u64>();
    let large_wasted: u64 = large_alignment_waste + page_table_usage(PageSize::Large(), large_page_count) * PageSize::Normal().size();
    let _large_usable: u64 = large_page_count * PageSize::Large().size();

    let normal_frames: heapless::Vec<MyMemoryDescriptor, HLU64> = conventional_memory.clone().map(|it| it.into()).collect();
    let normal_page_count: u64 = conventional_memory.map(|it| it.page_count).sum();
    let normal_wasted = page_table_usage(PageSize::Normal(), normal_page_count) * PageSize::Normal().size();
    let _normal_usable: u64 = normal_page_count * PageSize::Normal().size();

    let (recommended_page_size, final_page_count, waste) = if large_wasted < huge_wasted && large_wasted < normal_wasted { (PageSize::Large(), large_page_count, large_wasted) }
                                else if huge_wasted < large_wasted && huge_wasted < normal_wasted { (PageSize::Huge(), huge_page_count, huge_wasted) }
                                else if normal_wasted < huge_wasted && normal_wasted < large_wasted { (PageSize::Normal(), normal_page_count, normal_wasted) }
                                else { (PageSize::Large(), large_page_count, large_wasted) }; // If all page sizes waste the exact same amount of memory.

    info!("Recommended page size: {:?}, wasting {} and leaving {} of usable conventional memory",
        recommended_page_size,
        Byte::from_bytes(waste as u128).get_appropriate_unit(true),
        Byte::from_bytes(final_page_count as u128).get_appropriate_unit(true));

    fn print_waste(page_size: PageSize, waste: u64) { info!("\t{:?} would waste {} bytes", page_size, Byte::from_bytes(waste as u128).get_appropriate_unit(true)); }
    print_waste(PageSize::Huge(), huge_wasted);
    print_waste(PageSize::Large(), large_wasted);
    print_waste(PageSize::Normal(), normal_wasted);



    let frames: heapless::Vec<MyMemoryDescriptor, HLU64> = match recommended_page_size {
        PageSize::Huge() => huge_frames.clone().map(|it| it.0.as_ref().cloned()).filter_map(|it| it).collect(),
        PageSize::Large() => large_frames.clone().map(|it| it.0.as_ref().cloned()).filter_map(|it| it).collect(),
        PageSize::Normal() => normal_frames,
    };
    frames
}

unsafe fn mut_ptable_from_addr(addr: VirtAddr) -> &'static mut PageTable {
    &mut *(addr.as_u64() as *mut PageTable)
}


// Note that lookup is in the following order: lvl 4 table -> lvl 3 table -> lvl 2 table ->
// lvl 1 table -> mapped page
unsafe fn mmap<'a, F: FnMut() -> Result<PhysAddr, u32>, C: FnMut(PhysAddr) -> VirtAddr>(
    mut alloc_page: F,
    mut phys2virt: C,
    root_page_table: &mut PageTable,
    virt_start: VirtAddr,
    phys_start: PhysAddr,
    page_count: u64,
    page_size: PageSize,
    flags: PageTableFlags) {

    info!("mmap mapping {} pages to {:?}", page_count, virt_start);
    let mut table_flags = flags;

    for page in 0..page_count {
        let offset: usize = (page * page_size.size()) as usize;
        let page_virt_start = virt_start + offset;
        let page_phys_start = phys_start + offset;

        let p4i = page_virt_start.p4_index();
        let p3i = page_virt_start.p3_index();
        let p2i = page_virt_start.p2_index();
        let p1i = page_virt_start.p1_index();

        if let PageSize::Huge() | PageSize::Large() = page_size {
            table_flags.insert(PageTableFlags::HUGE_PAGE);
        }

        let entry = match page_size {
            PageSize::Huge() => {
                let p4e = &mut root_page_table[p4i];
                if p4e.is_unused() {
                    let new_page = alloc_page().unwrap();
                    mut_ptable_from_addr(phys2virt(new_page)).zero();
                    p4e.set_addr(new_page, table_flags);
                }

                let level3_table = mut_ptable_from_addr(phys2virt(p4e.addr()));
                let p3e = &mut level3_table[p3i];
                p3e
            }

            PageSize::Large() =>  {
                let p4e = &mut root_page_table[p4i];
                if p4e.is_unused() {
                    let new_page = alloc_page().unwrap();
                    mut_ptable_from_addr(phys2virt(new_page)).zero();
                    p4e.set_addr(new_page, table_flags);
                }

                let level3_table = mut_ptable_from_addr(phys2virt(p4e.addr()));
                let p3e = &mut level3_table[p3i];
                if p3e.is_unused() {
                    let new_page = alloc_page().unwrap();
                    mut_ptable_from_addr(phys2virt(new_page)).zero();
                    p3e.set_addr(new_page, table_flags);
                }

                let level2_table = mut_ptable_from_addr(phys2virt(p3e.addr()));
                let p2e = &mut level2_table[p2i];
                p2e
            }

            PageSize::Normal() => {
                let p4e = &mut root_page_table[p4i];
                if p4e.is_unused() {
                    let new_page = alloc_page().unwrap();
                    mut_ptable_from_addr(phys2virt(new_page)).zero();
                    p4e.set_addr(new_page, table_flags);
                }

                let level3_table = mut_ptable_from_addr(phys2virt(p4e.addr()));
                let p3e = &mut level3_table[p3i];
                if p3e.is_unused() {
                    let new_page = alloc_page().unwrap();
                    mut_ptable_from_addr(phys2virt(new_page)).zero();
                    p3e.set_addr(new_page, table_flags);
                }

                let level2_table = mut_ptable_from_addr(phys2virt(p3e.addr()));
                let p2e = &mut level2_table[p2i];
                if p2e.is_unused() {
                    let new_page = alloc_page().unwrap();
                    mut_ptable_from_addr(phys2virt(new_page)).zero();
                    p2e.set_addr(new_page, table_flags);
                }

                let level1_table = mut_ptable_from_addr(phys2virt(p2e.addr()));
                let p1e = &mut level1_table[p1i];
                p1e
            }
        };
        entry.set_addr(page_phys_start, flags);
    }
}

#[inline(never)]
unsafe fn remap(root_page_table: PhysAddr) {
    let frame: PhysFrame<Size4KiB> = PhysFrame::from_start_address(root_page_table).unwrap();
    //info!("Remap was called with return address = {:?}", VirtAddr::from_ptr(return_address()));
    Cr3::write(frame, Cr3Flags::empty());
    // Do we now need to jump for our dear life.. lest we get a pagefault?
    // ...yes we do
}


struct PhysFrameAllocator<I: Iterator<Item = UnusedPhysFrame>> {
    iter: I
}

impl<I: Iterator<Item = UnusedPhysFrame>> PhysFrameAllocator<I> {
    fn new(iter: I) -> Self {
        Self { iter }
    }
}

unsafe impl<I: Iterator<Item = UnusedPhysFrame>> FrameAllocator<Size4KiB> for PhysFrameAllocator<I> {
    fn allocate_frame(&mut self) -> Option<UnusedPhysFrame<Size4KiB>> {
        unsafe { self.iter.next() }
    }
}


unsafe fn frames_map_to<I, M, S, F>(
    mapper: &mut M,
    start: Page<S>,
    frames: I,
    flags: PageTableFlags,
    frame_allocator: &mut F) -> PageRangeInclusive<S>
        where   I: Iterator<Item = UnusedPhysFrame<S>>,
                S: x86_64::structures::paging::PageSize + core::fmt::Debug,
                M: Mapper<S>,
                F: FrameAllocator<Size4KiB> {
    let mut peekable = frames.peekable();

    let mut cursor = start;
    loop {
        let frame = match peekable.next() {
            Some(v) => v,
            None => break
        };

        mapper.map_to(cursor, frame, flags, frame_allocator).unwrap();

        if peekable.peek().is_some() {
            cursor = Page::from_start_address(VirtAddr::new(cursor.start_address().as_u64().checked_add(S::SIZE).unwrap())).unwrap();
        }
    }
    PageRangeInclusive {
        start: start,
        end: cursor
    }
}

unsafe fn ranges_map_to<I, M, S, F>(
    mapper: &mut M,
    start: Page<S>,
    ranges: I,
    flags: PageTableFlags,
    frame_allocator: &mut F) -> PageRangeInclusive<S>
        where  I: Iterator<Item = PhysFrameRangeInclusive<S>>,
        S: x86_64::structures::paging::PageSize + core::fmt::Debug,
        M: Mapper<S>,
        F: FrameAllocator<Size4KiB> {
    let mut peekable = ranges.peekable();
    assert!(peekable.peek().is_some());

    let mut end = start;
    for range in peekable {
        end = range_map_to(mapper, next_page(end), range, flags, frame_allocator).end;
    }
    PageRangeInclusive { start, end }
}


unsafe fn range_map_to<M, S, F>(
    mapper: &mut M,
    start: Page<S>,
    range: PhysFrameRangeInclusive<S>,
    flags: PageTableFlags,
    frame_allocator: &mut F) -> PageRangeInclusive<S>
        where  S: x86_64::structures::paging::PageSize + core::fmt::Debug,
        M: Mapper<S>,
        F: FrameAllocator<Size4KiB> {
    let frames = range.map(|it| UnusedPhysFrame::new(it));
    frames_map_to(mapper, start, frames, flags, frame_allocator)
}

fn mem_desc2frame_range(descriptor: &MemoryDescriptor) -> PhysFrameRangeInclusive {
    let start: PhysFrame<Size4KiB> = PhysFrame::from_start_address(PhysAddr::new(descriptor.phys_start)).unwrap();
    let end: PhysFrame<Size4KiB> = PhysFrame::from_start_address(PhysAddr::new(descriptor.phys_start.checked_add(Size4KiB::SIZE.checked_mul(descriptor.page_count).unwrap()).unwrap())).unwrap();
    PhysFrameRangeInclusive { start, end }
}

fn next_page<S>(page: Page<S>) -> Page<S> where S: x86_64::structures::paging::PageSize {
    let next_addr = VirtAddr::new(page.start_address().as_u64().checked_add(S::SIZE).unwrap());
    Page::from_start_address(next_addr).unwrap()
}

pub fn set_up_paging<'a, I>(mmap__: I) where I: Iterator<Item = &'a MemoryDescriptor> + Clone {
    let KERNELLAND:         VirtAddr = VirtAddr::new(0x800000000000);
    let MAPPED_PHYS_MEMORY: VirtAddr = VirtAddr::new(0x880000000000);

    // TODO join same-properties memory descriptors that are next to eachother
    let conventional_memory = mmap__.clone().filter(|it| it.ty == MemoryType::CONVENTIONAL);
    let frames = get_frames(conventional_memory);
    let get_normal_pages_phys = |frame: &MyMemoryDescriptor| {
        let modifier = match frame.page_size {
            PageSize::Huge() => PageSize::Huge().size() / PageSize::Normal().size(),
            PageSize::Large() => PageSize::Large().size() / PageSize::Normal().size(),
            PageSize::Normal() => 1,
        };
        let normal_page_count = frame.page_count * modifier;
        let cloned_frame = frame.clone();
        (0..normal_page_count).into_iter().map(move |i| cloned_frame.phys_start + i * PageSize::Normal().size())
    };

    let conventional_frames = frames.iter()
        .map(|frame| get_normal_pages_phys(frame))
        .flatten()
        .map(|it| unsafe { UnusedPhysFrame::new(PhysFrame::from_start_address(it).unwrap()) });
    let total_normal_memory = conventional_frames.clone().count() * 4096;



    let mut allocator = PhysFrameAllocator::new(conventional_frames.clone());

    // Valid until we remap as this depends on the identity mapped memory

    info!("Total available conventional memory: {}", Byte::from_bytes(total_normal_memory as u128).get_appropriate_unit(true));

    let phys2virt = |phys: PhysAddr| { VirtAddr::new(phys.as_u64()) };

    let root_table_addr = allocator.allocate_frame().unwrap().start_address();
    let root_table: &mut PageTable = unsafe { mut_ptable_from_addr(phys2virt(root_table_addr)) };
    root_table.zero();

    let mut mapper = unsafe { OffsetPageTable::new(root_table, VirtAddr::new(0)) };

    let flags = {
        let mut flags = PageTableFlags::empty();
        flags.insert(PageTableFlags::WRITABLE);
        flags.insert(PageTableFlags::NO_EXECUTE);
        flags
    };

    let codeflags = {
        let mut flags = PageTableFlags::empty();
        flags
    };

    let mmio_memory_frames = mmap__.clone().filter(|it| it.ty == MemoryType::MMIO).map(|desc| mem_desc2frame_range(desc));
    let mmio_port_memory_frames = mmap__.clone().filter(|it| it.ty == MemoryType::MMIO_PORT_SPACE).map(|desc| mem_desc2frame_range(desc));

    let acpi_memory_frames = mmap__.clone().filter(|it| it.ty == MemoryType::ACPI_RECLAIM).map(|desc| mem_desc2frame_range(desc));

    let kernel_code_frames = mmap__.clone().filter(|it| it.ty == MemoryType::LOADER_CODE).map(|desc| mem_desc2frame_range(desc));
    let kernel_data_frames = mmap__.clone().filter(|it| it.ty == MemoryType::LOADER_DATA).map(|desc| mem_desc2frame_range(desc));

    info!("Mapping physical memory..");
    unsafe { frames_map_to(&mut mapper, Page::from_start_address(MAPPED_PHYS_MEMORY).unwrap(), conventional_frames.clone(), flags, &mut allocator) };

    let mut last;

    info!("Mapping kernel code..");
    last = unsafe { ranges_map_to(&mut mapper, Page::from_start_address(KERNELLAND).unwrap(), kernel_code_frames, codeflags, &mut allocator) }.end;

    info!("Mapping kernel data to 0x{:X}..", next_page(last).start_address().as_u64());
    last = unsafe { ranges_map_to(&mut mapper, next_page(last), kernel_data_frames, flags, &mut allocator) }.end;


    // Create paging tables from 0x880000000000 onwards and map all conventional memory to
    // 0x88000000000 and onwards.
    /*let mut cursor = MAPPED_PHYS_MEMORY;
    info!("Mapping {} frames of physical conventional memory to 0x880000000000", frames.iter().count());
    for frame in frames.iter() {
        let new_virt_start = cursor;
        unsafe {
            info!("before mmap: frame.page_count = {}", frame.page_count);
            mmap(
                &mut alloc_page,
                phys2virt,
                root_table,
                new_virt_start,
                frame.phys_start,
                frame.page_count,
                frame.page_size,
                flags,
            );
        }
        info!("cursor: {:?}", cursor);
        cursor = VirtAddr::new(cursor.as_u64() + frame.page_size.size() * frame.page_count);
        info!("after cursor");
    }
    info!("Used {} pages for mapping all conventional memory to 0x88000000000 and onwards.", used_pages);*/



    /*let mut alloc_page = || { let retval = MAPPED_PHYS_MEMORY + used_pages * page_size; used_pages++; retval };
    {
        let mut cursor = KERNELLAND;
        for frame in kernel_code_frames {
            unsafe {
                mmap(

                );
            }
        }
    }*/

    // Technically the ACPI_RECLAIM regions are also reusable, but we don't reuse them right now.
    let reuse = mmap__.clone().filter(|it| match it.ty {
        MemoryType::BOOT_SERVICES_CODE => true,
        MemoryType::BOOT_SERVICES_DATA => true,
        _ => false,
    });

    // Amount of 4KiB pages of physical memory.
    let phys_page_count: u64 = mmap__.clone().map(|it| it.page_count).sum();

    unsafe {
        info!("Prepare for virtual memory remap!");
        remap(PhysAddr::new(root_table_addr.as_u64()));
    };
}
