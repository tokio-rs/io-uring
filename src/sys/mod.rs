#![allow(non_camel_case_types, non_upper_case_globals, dead_code, non_snake_case)]
#![allow(clippy::unreadable_literal, clippy::missing_safety_doc)]

use libc::*;

#[cfg(all(feature = "bindgen", not(feature = "overwrite")))]
include!(concat!(env!("OUT_DIR"), "/sys.rs"));

#[cfg(any(
    not(feature = "bindgen"),
    all(feature = "bindgen", feature = "overwrite")
))]
include!("sys.rs");

pub unsafe fn io_uring_register(fd: c_int, opcode: c_uint, arg: *const c_void, nr_args: c_uint)
    -> c_int
{
    syscall(
        __NR_io_uring_register as c_long,
        fd as c_long,
        opcode as c_long,
        arg as c_long,
        nr_args as c_long
    ) as _
}

pub unsafe fn io_uring_setup(entries: c_uint, p: *mut io_uring_params)
    -> c_int
{
    syscall(
        __NR_io_uring_setup as c_long,
        entries as c_long,
        p as c_long
    ) as _
}

pub unsafe fn io_uring_enter(fd: c_int, to_submit: c_uint, min_complete: c_uint, flags: c_uint, sig: *const sigset_t)
    -> c_int
{
    syscall(
        __NR_io_uring_enter as c_long,
        fd as c_long,
        to_submit as c_long,
        min_complete as c_long,
        flags as c_long,
        sig as c_long,
        core::mem::size_of::<sigset_t>() as c_long
    ) as _
}
