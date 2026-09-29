//! The two memory readings the pipeline needs, from the kernel. Kept thin;
//! the arithmetic is `available_bytes` in `engines/mod.rs`.

use super::{available_bytes, VmPages};
use crate::pipeline::Memory;

pub struct MacMemory;

impl Memory for MacMemory {
    fn footprint(&self) -> Option<u64> {
        footprint()
    }

    fn available(&self) -> Option<u64> {
        let total = crate::models::mac::memory_bytes().ok()?;
        Some(available_bytes(total, &vm_pages()?))
    }
}

/// This process's physical footprint, the number Activity Monitor shows as
/// Memory. It includes Metal buffers.
pub fn footprint() -> Option<u64> {
    rusage().map(|info| info.ri_phys_footprint)
}

/// The largest footprint this process has had.
pub fn peak_footprint() -> Option<u64> {
    rusage().map(|info| info.ri_lifetime_max_phys_footprint)
}

fn rusage() -> Option<libc::rusage_info_v4> {
    // SAFETY: `info` is plain data of the size RUSAGE_INFO_V4 writes.
    unsafe {
        let mut info: libc::rusage_info_v4 = std::mem::zeroed();
        let status = libc::proc_pid_rusage(
            libc::getpid(),
            libc::RUSAGE_INFO_V4,
            (&mut info as *mut libc::rusage_info_v4).cast(),
        );
        (status == 0).then_some(info)
    }
}

fn vm_pages() -> Option<VmPages> {
    // SAFETY: `stats` and `count` describe a vm_statistics64 buffer, which
    // HOST_VM_INFO64 fills up to `count` fields.
    unsafe {
        let mut stats: libc::vm_statistics64 = std::mem::zeroed();
        let mut count = libc::HOST_VM_INFO64_COUNT;
        #[allow(deprecated)]
        let host = libc::mach_host_self();
        let status = libc::host_statistics64(
            host,
            libc::HOST_VM_INFO64,
            (&mut stats as *mut libc::vm_statistics64).cast(),
            &mut count,
        );
        if status != libc::KERN_SUCCESS {
            return None;
        }
        #[allow(deprecated)]
        let page_size = libc::vm_page_size as u64;
        Some(VmPages {
            page_size,
            wired: stats.wire_count.into(),
            compressed: stats.compressor_page_count.into(),
            anonymous: stats.internal_page_count.into(),
            purgeable: stats.purgeable_count.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readings_are_plausible_for_this_process_and_mac() {
        let now = footprint().unwrap();
        assert!(now > 1 << 20, "footprint {now}");
        assert!(peak_footprint().unwrap() >= now);
        let total = crate::models::mac::memory_bytes().unwrap();
        let available = MacMemory.available().unwrap();
        assert!(available > 0 && available < total, "{available} of {total}");
    }
}
