//! Common Linux types not provided by libc.

pub(crate) mod sealed {
    use super::{Fd, Fixed};
    use std::os::unix::io::RawFd;

    #[derive(Debug)]
    pub enum Target {
        Fd(RawFd),
        Fixed(u32),
    }

    pub trait UseFd: Sized {
        fn into(self) -> RawFd;
    }

    pub trait UseFixed: Sized {
        fn into(self) -> Target;
    }

    impl UseFd for Fd {
        #[inline]
        fn into(self) -> RawFd {
            self.0
        }
    }

    impl UseFixed for Fd {
        #[inline]
        fn into(self) -> Target {
            Target::Fd(self.0)
        }
    }

    impl UseFixed for Fixed {
        #[inline]
        fn into(self) -> Target {
            Target::Fixed(self.0)
        }
    }
}

use crate::sys;
use bitflags::bitflags;
use std::os::unix::io::RawFd;

#[cfg(feature = "unstable")]
use std::marker::PhantomData;

#[cfg(feature = "unstable")]
use crate::util::cast_ptr;

pub use sys::__kernel_rwf_t as RwFlags;

/// Opaque types, you should use [`statx`](struct@libc::statx) instead.
#[repr(C)]
pub struct statx {
    _priv: (),
}

/// Opaque types, you should use [`epoll_event`](libc::epoll_event) instead.
#[repr(C)]
pub struct epoll_event {
    _priv: (),
}

/// A file descriptor that has not been registered with io_uring.
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct Fd(pub RawFd);

/// A file descriptor that has been registered with io_uring using
/// [`Submitter::register_files`](crate::Submitter::register_files). This can reduce overhead
/// compared to using [`Fd`] in some cases.
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct Fixed(pub u32);

bitflags! {
    /// Options for [`Timeout`](super::Timeout).
    pub struct TimeoutFlags: u32 {
        const ABS = sys::IORING_TIMEOUT_ABS;

        #[cfg(feature = "unstable")]
        const UPDATE = sys::IORING_TIMEOUT_UPDATE;
    }
}

bitflags! {
    /// Options for [`Fsync`](super::Fsync).
    pub struct FsyncFlags: u32 {
        const DATASYNC = sys::IORING_FSYNC_DATASYNC;
    }
}

/// Wrapper around `open_how` as used in [the `openat2(2)` system
/// call](https://man7.org/linux/man-pages/man2/openat2.2.html).
#[derive(Default, Debug, Clone, Copy)]
#[repr(transparent)]
pub struct OpenHow(sys::open_how);

impl OpenHow {
    pub const fn new() -> Self {
        OpenHow(sys::open_how {
            flags: 0,
            mode: 0,
            resolve: 0,
        })
    }

    pub const fn flags(mut self, flags: u64) -> Self {
        self.0.flags = flags;
        self
    }

    pub const fn mode(mut self, mode: u64) -> Self {
        self.0.mode = mode;
        self
    }

    pub const fn resolve(mut self, resolve: u64) -> Self {
        self.0.resolve = resolve;
        self
    }
}

#[derive(Default, Debug, Clone, Copy)]
#[repr(transparent)]
pub struct Timespec(sys::__kernel_timespec);

impl Timespec {
    #[inline]
    pub const fn new() -> Self {
        Timespec(sys::__kernel_timespec {
            tv_sec: 0,
            tv_nsec: 0,
        })
    }

    #[inline]
    pub const fn sec(mut self, sec: u64) -> Self {
        self.0.tv_sec = sec as _;
        self
    }

    #[inline]
    pub const fn nsec(mut self, nsec: u32) -> Self {
        self.0.tv_nsec = nsec as _;
        self
    }
}

/// Submit arguments
///
/// Note that arguments that exceed their lifetime will fail to compile.
///
/// ```compile_fail
/// use io_uring::types::{ SubmitArgs, Timespec };
///
/// let sigmask: libc::sigset_t = unsafe { std::mem::zeroed() };
///
/// let mut args = SubmitArgs::new();
///
/// {
///     let ts = Timespec::new();
///     args = args.timespec(&ts);
///     args = args.sigmask(&sigmask);
/// }
///
/// drop(args);
/// ```
#[cfg(feature = "unstable")]
#[derive(Default, Debug, Clone, Copy)]
pub struct SubmitArgs<'prev: 'now, 'now> {
    pub(crate) args: sys::io_uring_getevents_arg,
    prev: PhantomData<&'prev ()>,
    now: PhantomData<&'now ()>,
}

#[cfg(feature = "unstable")]
impl<'prev, 'now> SubmitArgs<'prev, 'now> {
    #[inline]
    pub const fn new() -> SubmitArgs<'static, 'static> {
        let args = sys::io_uring_getevents_arg {
            sigmask: 0,
            sigmask_sz: 0,
            pad: 0,
            ts: 0,
        };

        SubmitArgs {
            args,
            prev: PhantomData,
            now: PhantomData,
        }
    }

    #[inline]
    pub fn sigmask<'new>(mut self, sigmask: &'new libc::sigset_t) -> SubmitArgs<'now, 'new> {
        self.args.sigmask = cast_ptr(sigmask) as _;
        self.args.sigmask_sz = std::mem::size_of::<libc::sigset_t>() as _;

        SubmitArgs {
            args: self.args,
            prev: self.now,
            now: PhantomData,
        }
    }

    #[inline]
    pub fn timespec<'new>(mut self, timespec: &'new Timespec) -> SubmitArgs<'now, 'new> {
        self.args.ts = cast_ptr(timespec) as _;

        SubmitArgs {
            args: self.args,
            prev: self.now,
            now: PhantomData,
        }
    }
}
