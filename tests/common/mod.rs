#![allow(dead_code, unused_macros)]

mod ioop;

use io_uring::IoUring;
pub use ioop::do_write_read;


include!("../../src/util.rs");

pub enum KernelSupport {
    Less,
    V54,
    V55,
    V56
}

pub fn check_kernel_support(ring: &IoUring) -> KernelSupport {
    let p = ring.params();

    if !p.is_feature_single_mmap() {
        return KernelSupport::Less;
    }

    if !p.is_feature_nodrop() {
        return KernelSupport::V54;
    }

    KernelSupport::V55
}
