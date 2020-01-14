#![allow(dead_code)]

mod ioop;

use linux_io_uring::IoUring;
use linux_io_uring::opcode::{ self, types };
pub use ioop::do_write_read;


include!("../../src/util.rs");

pub fn one_s() -> opcode::Timeout {
    static ONE_S: types::Timespec = types::Timespec { tv_sec: 1, tv_nsec: 0 };

    opcode::Timeout::new(&ONE_S)
}

#[cfg(feature = "unstable")]
pub fn is_stable_kernel(ring: &IoUring) -> bool {
    !ring.params().is_feature_nodrop()
}

#[cfg(not(feature = "unstable"))]
pub fn is_stable_kernel(_ring: &IoUring) -> bool {
    true
}
