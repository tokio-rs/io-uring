#![allow(
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    non_snake_case,
    unused_qualifications
)]
#![allow(clippy::unreadable_literal, clippy::missing_safety_doc)]

use libc::*;

#[cfg(all(feature = "bindgen", not(feature = "overwrite")))]
include!(concat!(env!("OUT_DIR"), "/sys.rs"));

#[cfg(any(
    not(feature = "bindgen"),
    all(feature = "bindgen", feature = "overwrite")
))]
include!("sys.rs");

#[cfg(not(feature = "direct-syscall"))]
pub unsafe fn io_uring_register(
    fd: c_int,
    opcode: c_uint,
    arg: *const c_void,
    nr_args: c_uint,
) -> c_int {
    syscall(
        if cfg!(feature = "bindgen") {
            __NR_io_uring_register as c_long
        } else {
            libc::SYS_io_uring_register
        },
        fd as c_long,
        opcode as c_long,
        arg as c_long,
        nr_args as c_long,
    ) as _
}

#[cfg(feature = "direct-syscall")]
pub unsafe fn io_uring_register(
    fd: c_int,
    opcode: c_uint,
    arg: *const c_void,
    nr_args: c_uint,
) -> c_int {
    sc::syscall4(
        if cfg!(feature = "bindgen") {
            __NR_io_uring_register as usize
        } else {
            libc::SYS_io_uring_register as usize
        },
        fd as usize,
        opcode as usize,
        arg as usize,
        nr_args as usize,
    ) as _
}

#[cfg(not(feature = "direct-syscall"))]
pub unsafe fn io_uring_setup(entries: c_uint, p: *mut io_uring_params) -> c_int {
    syscall(
        if cfg!(feature = "bindgen") {
            __NR_io_uring_setup as c_long
        } else {
            libc::SYS_io_uring_setup
        },
        entries as c_long,
        p as c_long,
    ) as _
}

#[cfg(feature = "direct-syscall")]
pub unsafe fn io_uring_setup(entries: c_uint, p: *mut io_uring_params) -> c_int {
    sc::syscall2(
        if cfg!(feature = "bindgen") {
            __NR_io_uring_setup as usize
        } else {
            libc::SYS_io_uring_setup as usize
        },
        entries as usize,
        p as usize,
    ) as _
}

#[cfg(not(feature = "direct-syscall"))]
pub unsafe fn io_uring_enter(
    fd: c_int,
    to_submit: c_uint,
    min_complete: c_uint,
    flags: c_uint,
    arg: *const libc::c_void,
    size: usize,
) -> c_int {
    syscall(
        if cfg!(feature = "bindgen") {
            __NR_io_uring_enter as c_long
        } else {
            libc::SYS_io_uring_enter
        },
        fd as c_long,
        to_submit as c_long,
        min_complete as c_long,
        flags as c_long,
        arg as c_long,
        size as c_long,
    ) as _
}

#[cfg(feature = "direct-syscall")]
pub unsafe fn io_uring_enter(
    fd: c_int,
    to_submit: c_uint,
    min_complete: c_uint,
    flags: c_uint,
    arg: *const libc::c_void,
    size: usize,
) -> c_int {
    sc::syscall6(
        if cfg!(feature = "bindgen") {
            __NR_io_uring_enter as usize
        } else {
            libc::SYS_io_uring_enter as usize
        },
        fd as usize,
        to_submit as usize,
        min_complete as usize,
        flags as usize,
        arg as usize,
        size,
    ) as _
}
