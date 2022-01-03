//! Operation codes that can be used to construct [`squeue::Entry`](crate::squeue::Entry)s.

#![allow(clippy::new_without_default)]

use std::mem;
use std::os::unix::io::RawFd;

use crate::squeue::Entry;
use crate::sys;
use crate::types::{self, sealed};

/// Trait to prepare an SQE from an opcode object.
pub trait PrepareSQE {
    /// Prepare an SQE from an opcode object.
    fn prepare(&self, sqe: &mut sys::io_uring_sqe);
}

trait AsSqeValue {
    type T;
    fn as_sqe_value(&self) -> Self::T;
}

impl AsSqeValue for sealed::Target {
    type T = i32;

    fn as_sqe_value(&self) -> Self::T {
        match self {
            sealed::Target::Fd(fd) => *fd as _,
            sealed::Target::Fixed(i) => *i as _,
        }
    }
}

impl AsSqeValue for types::FsyncFlags {
    type T = u32;

    fn as_sqe_value(&self) -> Self::T {
        self.bits()
    }
}

impl AsSqeValue for types::TimeoutFlags {
    type T = u32;

    fn as_sqe_value(&self) -> Self::T {
        self.bits()
    }
}

impl AsSqeValue for u64 {
    type T = u64;

    fn as_sqe_value(&self) -> Self::T {
        *self as Self::T
    }
}

impl AsSqeValue for i64 {
    type T = u64;

    fn as_sqe_value(&self) -> Self::T {
        *self as Self::T
    }
}

impl AsSqeValue for u32 {
    type T = u32;

    fn as_sqe_value(&self) -> Self::T {
        *self
    }
}

impl AsSqeValue for i32 {
    type T = i32;

    fn as_sqe_value(&self) -> Self::T {
        *self
    }
}

impl AsSqeValue for u16 {
    type T = u16;

    fn as_sqe_value(&self) -> Self::T {
        *self
    }
}

impl<U> AsSqeValue for *const U {
    type T = u64;

    fn as_sqe_value(&self) -> Self::T {
        *self as _
    }
}

impl<U> AsSqeValue for *mut U {
    type T = u64;

    fn as_sqe_value(&self) -> Self::T {
        *self as _
    }
}

macro_rules! set_fd_flags {
    ( $sqe:ident, $opfd:expr ) => {
        if matches!($opfd, sealed::Target::Fixed(_)) {
            $sqe.flags |= crate::squeue::Flags::FIXED_FILE.bits();
        }
    };
    ( $self:ident . $sqe:ident, $opfd:expr ) => {
        if matches!($opfd, sealed::Target::Fixed(_)) {
            $self.$sqe.flags |= crate::squeue::Flags::FIXED_FILE.bits();
        }
    };
}

macro_rules! opcode {
    (@type impl sealed::UseFixed ) => {
        sealed::Target
    };
    (@type impl sealed::UseFd ) => {
        RawFd
    };
    (@type $name:ty ) => {
        $name
    };
    (
        $( #[$outer:meta] )*
        pub struct $name:ident {
            $( #[$new_meta:meta] )*

            $( $field:ident : { $( $tnt:tt )+ } => sqe $(. $sqe_field:ident)+ ),*

            $(,)?

            ;;

            $(
                $( #[$opt_meta:meta] )*
                $opt_field:ident : $opt_tname:ty = $default:expr => sqe $(. $opt_sqe_field:ident)+
            ),*

            $(,)?
        }

        $( #[$outer_sqe:meta] )*
        pub struct $name_sqe:ident { }

        pub const CODE = $opcode:expr;

        fn set_special_fields(&mut $self:ident, $sqe1:ident : &mut sys::io_uring_sqe) {
            $( op: $set_op_special_fields:block )?
            $( op_sqe: $set_op_sqe_special_fields:block )?
        }
    ) => {
        $( #[$outer] )*
        pub struct $name {
            $( $field : opcode!(@type $( $tnt )*), )*
            $( $opt_field : $opt_tname, )*
        }

        impl $name {
            /// The opcode of the operation. This can be passed to
            /// [`Probe::is_supported`](crate::Probe::is_supported) to check if this operation is
            /// supported with the current kernel.
            pub const CODE: u8 = $opcode as _;

            $( #[$new_meta] )*
            #[inline]
            pub fn new($( $field : $( $tnt )* ),*) -> Self {
                $name {
                    $( $field: $field.into(), )*
                    $( $opt_field: $default, )*
                }
            }

            $(
                $( #[$opt_meta] )*
                #[inline]
                pub const fn $opt_field(mut self, $opt_field: $opt_tname) -> Self {
                    self.$opt_field = $opt_field;
                    self
                }
            )*

            #[inline]
            pub fn build(self) -> Entry {
                let mut sqe = sqe_zeroed();
                self.prepare(&mut sqe);
                Entry(sqe)
            }
        }

        impl PrepareSQE for $name {
            #[inline]
            fn prepare(& $self, $sqe1: &mut sys::io_uring_sqe) {
                $sqe1 .opcode = $name::CODE;
                $( $sqe1 $(. $sqe_field)* = $self. $field .as_sqe_value() as _; )*
                $( $sqe1 $(. $opt_sqe_field)* = $self. $opt_field .as_sqe_value() as _; )*
                $( $set_op_special_fields )?
            }
        }

        $( #[$outer_sqe] )*
        #[repr(transparent)]
        pub struct $name_sqe {
            sqe: sys::io_uring_sqe,
        }

        impl $name_sqe {
            /// The opcode of the operation. This can be passed to
            /// [`Probe::is_supported`](crate::Probe::is_supported) to check if this operation is
            /// supported with the current kernel.
            pub const CODE: u8 = $opcode as _;

            #[inline]
            pub fn prepare(&mut $self, $( $field : opcode!(@type $( $tnt )*), )* ) {
                $self.sqe.opcode = Self::CODE;
                $( $self.sqe $(. $sqe_field)* = $field .as_sqe_value() as _; )*
                $( $self.sqe $(. $opt_sqe_field)* = $default .as_sqe_value() as _; )*
                $( $set_op_sqe_special_fields )?
            }

            #[inline]
            pub fn get_mut_sqe(&mut self) -> &mut sys::io_uring_sqe {
                &mut self.sqe
            }

            $(
                $( #[$opt_meta] )*
                #[inline]
                pub fn $opt_field(mut self, $opt_field: $opt_tname) -> Self {
                    self.sqe$(. $opt_sqe_field )* = $opt_field .as_sqe_value() as _;
                    self
                }
            )*
        }

        impl<'a> From<&'a mut sys::io_uring_sqe> for &'a mut $name_sqe {
            #[inline]
            fn from(sqe: &'a mut sys::io_uring_sqe) -> &'a mut $name_sqe {
                unsafe {
                    mem::transmute(sqe)
                }
            }
        }
    }
}

/// inline zeroed to improve codegen
#[inline(always)]
pub(crate) fn sqe_zeroed() -> sys::io_uring_sqe {
    unsafe { std::mem::zeroed() }
}

opcode!(
    /// Do not perform any I/O.
    ///
    /// This is useful for testing the performance of the io_uring implementation itself.
    #[derive(Debug)]
    pub struct Nop { ;; }

    /// Do not perform any I/O.
    ///
    /// This is useful for testing the performance of the io_uring implementation itself.
    pub struct NopSqe { }

    pub const CODE = sys::IORING_OP_NOP;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { sqe.fd = -1; }
        op_sqe: { self.sqe.fd = -1; }
    }
);

opcode!(
    /// Vectored read, equivalent to `preadv2(2)`.
    #[derive(Debug)]
    pub struct Readv {
        fd: { impl sealed::UseFixed } => sqe.fd,
        iovec: { *const libc::iovec } => sqe.__bindgen_anon_2.addr,
        len: { u32 } => sqe.len,
        ;;
        ioprio: u16 = 0u16 => sqe.ioprio,
        offset: libc::off_t = 0i64 => sqe.__bindgen_anon_1.off,
        /// specified for read operations, contains a bitwise OR of per-I/O flags,
        /// as described in the `preadv2(2)` man page.
        rw_flags: types::RwFlags = 0i32 => sqe.__bindgen_anon_3.rw_flags,
    }

    /// Vectored read, equivalent to `preadv2(2)`.
    pub struct ReadvSqe { }

    pub const CODE = sys::IORING_OP_READV;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { set_fd_flags!(sqe, self.fd); }
        op_sqe: { set_fd_flags!(self.sqe, fd); }
    }
);

opcode!(
    /// Vectored write, equivalent to `pwritev2(2)`.
    #[derive(Debug)]
    pub struct Writev {
        fd: { impl sealed::UseFixed } => sqe.fd,
        iovec: { *const libc::iovec } => sqe.__bindgen_anon_2.addr,
        len: { u32 } => sqe.len,
        ;;
        ioprio: u16 = 0u16 => sqe.ioprio,
        offset: libc::off_t = 0i64 => sqe.__bindgen_anon_1.off,
        /// specified for write operations, contains a bitwise OR of per-I/O flags,
        /// as described in the `preadv2(2)` man page.
        rw_flags: types::RwFlags = 0i32 => sqe.__bindgen_anon_3.rw_flags,
    }

    /// Vectored write, equivalent to `pwritev2(2)`.
    pub struct WritevSqe { }

    pub const CODE = sys::IORING_OP_WRITEV;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { set_fd_flags!(sqe, self.fd); }
        op_sqe: { set_fd_flags!(self.sqe, fd); }
    }
);

opcode!(
    /// File sync, equivalent to `fsync(2)`.
    ///
    /// Note that, while I/O is initiated in the order in which it appears in the submission queue,
    /// completions are unordered. For example, an application which places a write I/O followed by
    /// an fsync in the submission queue cannot expect the fsync to apply to the write. The two
    /// operations execute in parallel, so the fsync may complete before the write is issued to the
    /// storage. The same is also true for previously issued writes that have not completed prior to
    /// the fsync.
    #[derive(Debug)]
    pub struct Fsync {
        fd: { impl sealed::UseFixed } => sqe.fd,
        ;;
        /// The `flags` bit mask may contain either 0, for a normal file integrity sync,
        /// or [types::FsyncFlags::DATASYNC] to provide data sync only semantics.
        /// See the descriptions of `O_SYNC` and `O_DSYNC` in the `open(2)` manual page for more information.
        flags: types::FsyncFlags = types::FsyncFlags::empty() => sqe.__bindgen_anon_3.fsync_flags,
    }

    /// File sync, equivalent to `fsync(2)`.
    ///
    /// Note that, while I/O is initiated in the order in which it appears in the submission queue,
    /// completions are unordered. For example, an application which places a write I/O followed by
    /// an fsync in the submission queue cannot expect the fsync to apply to the write. The two
    /// operations execute in parallel, so the fsync may complete before the write is issued to the
    /// storage. The same is also true for previously issued writes that have not completed prior to
    /// the fsync.
    pub struct FsyncSqe { }

    pub const CODE = sys::IORING_OP_FSYNC;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { set_fd_flags!(sqe, self.fd); }
        op_sqe: { set_fd_flags!(self.sqe, fd); }
    }
);

opcode!(
    /// Read from pre-mapped buffers that have been previously registered with
    /// [`Submitter::register_buffers`](crate::Submitter::register_buffers).
    ///
    /// The return values match those documented in the `preadv2(2)` man pages.
    #[derive(Debug)]
    pub struct ReadFixed {
        /// The `buf_index` is an index into an array of fixed buffers,
        /// and is only valid if fixed buffers were registered.
        fd: { impl sealed::UseFixed } => sqe.fd,
        buf: { *mut u8 } => sqe.__bindgen_anon_2.addr,
        len: { u32 } => sqe.len,
        buf_index: { u16 } => sqe.__bindgen_anon_4.buf_index,
        ;;
        offset: libc::off_t = 0i64 => sqe.__bindgen_anon_1.off,
        ioprio: u16 = 0u16 => sqe.ioprio,
        /// specified for read operations, contains a bitwise OR of per-I/O flags,
        /// as described in the `preadv2(2)` man page.
        rw_flags: types::RwFlags = 0i32 => sqe.__bindgen_anon_3.rw_flags,
    }

    /// Read from pre-mapped buffers that have been previously registered with
    /// [`Submitter::register_buffers`](crate::Submitter::register_buffers).
    ///
    /// The return values match those documented in the `preadv2(2)` man pages.
    pub struct ReadFixedSqe { }

    pub const CODE = sys::IORING_OP_READ_FIXED;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { set_fd_flags!(sqe, self.fd); }
        op_sqe: { set_fd_flags!(self.sqe, fd); }
    }
);

opcode!(
    /// Write to pre-mapped buffers that have been previously registered with
    /// [`Submitter::register_buffers`](crate::Submitter::register_buffers).
    ///
    /// The return values match those documented in the `pwritev2(2)` man pages.
    #[derive(Debug)]
    pub struct WriteFixed {
        /// The `buf_index` is an index into an array of fixed buffers,
        /// and is only valid if fixed buffers were registered.
        fd: { impl sealed::UseFixed } => sqe.fd,
        buf: { *const u8 } => sqe.__bindgen_anon_2.addr,
        len: { u32 } => sqe.len,
        buf_index: { u16 } => sqe.__bindgen_anon_4.buf_index,
        ;;
        ioprio: u16 = 0u16 => sqe.ioprio,
        offset: libc::off_t = 0i64 => sqe.__bindgen_anon_1.off,
        /// specified for write operations, contains a bitwise OR of per-I/O flags,
        /// as described in the `preadv2(2)` man page.
        rw_flags: types::RwFlags = 0i32 => sqe.__bindgen_anon_3.rw_flags,
    }

    /// Write to pre-mapped buffers that have been previously registered with
    /// [`Submitter::register_buffers`](crate::Submitter::register_buffers).
    ///
    /// The return values match those documented in the `pwritev2(2)` man pages.
    pub struct WriteFixedSqe { }

    pub const CODE = sys::IORING_OP_WRITE_FIXED;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { set_fd_flags!(sqe, self.fd); }
        op_sqe: { set_fd_flags!(self.sqe, fd); }
    }
);

opcode!(
    /// Poll the specified fd.
    ///
    /// Unlike poll or epoll without `EPOLLONESHOT`, this interface always works in one shot mode.
    /// That is, once the poll operation is completed, it will have to be resubmitted.
    #[derive(Debug)]
    pub struct PollAdd {
        /// The bits that may be set in `flags` are defined in `<poll.h>`,
        /// and documented in `poll(2)`.
        fd: { impl sealed::UseFixed } => sqe.fd,
        flags: { u32 } => sqe.__bindgen_anon_3.poll32_events
        ;;
    }

    /// Poll the specified fd.
    ///
    /// Unlike poll or epoll without `EPOLLONESHOT`, this interface always works in one shot mode.
    /// That is, once the poll operation is completed, it will have to be resubmitted.
    pub struct PollAddSqe { }

    pub const CODE = sys::IORING_OP_POLL_ADD;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: {
            set_fd_flags!(sqe, self.fd);
            #[cfg(target_endian = "big")] {
                let x = self.flags << 16;
                let y = self.flags >> 16;
                let flags = x | y;
                sqe.__bindgen_anon_3.poll32_events = flags;
            }
        }
        op_sqe: {
            set_fd_flags!(self.sqe, fd);
            #[cfg(target_endian = "big")] {
                let x = flags << 16;
                let y = flags >> 16;
                let flags = x | y;
                self.sqe.__bindgen_anon_3.poll32_events = flags;
            }
        }
    }
);

opcode!(
    /// Remove an existing [poll](PollAdd) request.
    ///
    /// If found, the `result` method of the `cqueue::Entry` will return 0.
    /// If not found, `result` will return `-libc::ENOENT`.
    #[derive(Debug)]
    pub struct PollRemove {
        user_data: { u64 } => sqe.__bindgen_anon_2.addr,
        ;;
    }

    /// Remove an existing [poll](PollAdd) request.
    ///
    /// If found, the `result` method of the `cqueue::Entry` will return 0.
    /// If not found, `result` will return `-libc::ENOENT`.
    pub struct PollRemoveSqe { }

    pub const CODE = sys::IORING_OP_POLL_REMOVE;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { sqe.fd = -1; }
        op_sqe: { self.sqe.fd = -1; }
    }
);

opcode!(
    /// Sync a file segment with disk, equivalent to `sync_file_range(2)`.
    #[derive(Debug)]
    pub struct SyncFileRange {
        fd: { impl sealed::UseFixed } => sqe.fd,
        len: { u32 } => sqe.len,
        ;;
        /// the offset method holds the offset in bytes
        offset: libc::off64_t = 0i64 => sqe.__bindgen_anon_1.off,
        /// the flags method holds the flags for the command
        flags: u32 = 0u32 => sqe.__bindgen_anon_3.sync_range_flags,
    }

    /// Sync a file segment with disk, equivalent to `sync_file_range(2)`.
    pub struct SyncFileRangeSqe { }

    pub const CODE = sys::IORING_OP_SYNC_FILE_RANGE;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { set_fd_flags!(sqe, self.fd); }
        op_sqe: { set_fd_flags!(self.sqe, fd); }
    }
);

opcode!(
    /// Send a message on a socket, equivalent to `send(2)`.
    ///
    /// fd must be set to the socket file descriptor, addr must contains a pointer to the msghdr
    /// structure, and flags holds the flags associated with the system call.
    #[derive(Debug)]
    pub struct SendMsg {
        fd: { impl sealed::UseFixed } => sqe.fd,
        msg: { *const libc::msghdr } => sqe.__bindgen_anon_2.addr,
        ;;
        ioprio: u16 = 0u16 => sqe.ioprio,
        flags: u32 = 0u32 => sqe.__bindgen_anon_3.msg_flags,
    }

    /// Send a message on a socket, equivalent to `send(2)`.
    ///
    /// fd must be set to the socket file descriptor, addr must contains a pointer to the msghdr
    /// structure, and flags holds the flags associated with the system call.
    pub struct SendMsgSqe { }

    pub const CODE = sys::IORING_OP_SENDMSG;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: {
            set_fd_flags!(sqe, self.fd);
            sqe.len = 1;
        }
        op_sqe: {
            set_fd_flags!(self.sqe, fd);
            self.sqe.len = 1;
        }
    }
);

opcode!(
    /// Receive a message on a socket, equivalent to `recvmsg(2)`.
    ///
    /// See also the description of [`SendMsg`].
    #[derive(Debug)]
    pub struct RecvMsg {
        fd: { impl sealed::UseFixed } => sqe.fd,
        msg: { *mut libc::msghdr } => sqe.__bindgen_anon_2.addr,
        ;;
        ioprio: u16 = 0u16 => sqe.ioprio,
        flags: u32 = 0u32 => sqe.__bindgen_anon_3.msg_flags,
    }

    /// Receive a message on a socket, equivalent to `recvmsg(2)`.
    ///
    /// See also the description of [`SendMsg`].
    pub struct RecvMsgSqe { }

    pub const CODE = sys::IORING_OP_RECVMSG;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: {
            set_fd_flags!(sqe, self.fd);
            sqe.len = 1;
        }
        op_sqe: {
            set_fd_flags!(self.sqe, fd);
            self.sqe.len = 1;
        }
    }
);

opcode!(
    /// Register a timeout operation.
    ///
    /// A timeout will trigger a wakeup event on the completion ring for anyone waiting for events.
    /// A timeout condition is met when either the specified timeout expires, or the specified number of events have completed.
    /// Either condition will trigger the event.
    /// The request will complete with `-ETIME` if the timeout got completed through expiration of the timer,
    /// or 0 if the timeout got completed through requests completing on their own.
    /// If the timeout was cancelled before it expired, the request will complete with `-ECANCELED`.
    #[derive(Debug)]
    pub struct Timeout {
        timespec: { *const types::Timespec } => sqe.__bindgen_anon_2.addr,
        ;;
        /// `count` may contain a completion event count.
        count: u32 = 0u32 => sqe.__bindgen_anon_1.off,

        /// `flags` may contain [types::TimeoutFlags::ABS] for an absolute timeout value, or 0 for a relative timeout.
        flags: types::TimeoutFlags = types::TimeoutFlags::empty() => sqe.__bindgen_anon_3.timeout_flags,
    }

    /// Register a timeout operation.
    ///
    /// A timeout will trigger a wakeup event on the completion ring for anyone waiting for events.
    /// A timeout condition is met when either the specified timeout expires, or the specified number of events have completed.
    /// Either condition will trigger the event.
    /// The request will complete with `-ETIME` if the timeout got completed through expiration of the timer,
    /// or 0 if the timeout got completed through requests completing on their own.
    /// If the timeout was cancelled before it expired, the request will complete with `-ECANCELED`.
    pub struct TimeoutSqe { }

    pub const CODE = sys::IORING_OP_TIMEOUT;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: {
            sqe.fd = -1;
            sqe.len = 1;
        }
        op_sqe: {
            let sqe = &mut self.sqe;
            sqe.fd = -1;
            sqe.len = 1;
        }
    }
);

// === 5.5 ===

opcode!(
    /// Attempt to remove an existing [timeout operation](Timeout).
    pub struct TimeoutRemove {
        user_data: { u64 } => sqe.__bindgen_anon_2.addr,
        ;;
        flags: types::TimeoutFlags = types::TimeoutFlags::empty() => sqe.__bindgen_anon_3.timeout_flags,
    }

    /// Attempt to remove an existing [timeout operation](Timeout).
    pub struct TimeoutRemoveSqe { }

    pub const CODE = sys::IORING_OP_TIMEOUT_REMOVE;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { sqe.fd = -1; }
        op_sqe: { self.sqe.fd = -1; }
    }
);

opcode!(
    /// Accept a new connection on a socket, equivalent to `accept4(2)`.
    pub struct Accept {
        fd: { impl sealed::UseFixed } => sqe.fd,
        addr: { *mut libc::sockaddr } => sqe.__bindgen_anon_2.addr,
        addrlen: { *mut libc::socklen_t } => sqe.__bindgen_anon_1.addr2,
        ;;
        flags: i32 = 0i32 => sqe.__bindgen_anon_3.accept_flags,
    }

    /// Accept a new connection on a socket, equivalent to `accept4(2)`.
    pub struct AcceptSqe { }

    pub const CODE = sys::IORING_OP_ACCEPT;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { set_fd_flags!(sqe, self.fd); }
        op_sqe: { set_fd_flags!(self.sqe, fd); }
    }
);

opcode!(
    /// Attempt to cancel an already issued request.
    pub struct AsyncCancel {
        user_data: { u64 } => sqe.__bindgen_anon_2.addr,
        ;;

        // TODO flags
    }

    /// Attempt to cancel an already issued request.
    pub struct AsyncCancelSqe { }

    pub const CODE = sys::IORING_OP_ASYNC_CANCEL;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { sqe.fd = -1; }
        op_sqe: { self.sqe.fd = -1; }
    }
);

opcode!(
    /// This request must be linked with another request through
    /// [`Flags::IO_LINK`](crate::squeue::Flags::IO_LINK) which is described below.
    /// Unlike [`Timeout`], [`LinkTimeout`] acts on the linked request, not the completion queue.
    pub struct LinkTimeout {
        timespec: { *const types::Timespec } => sqe.__bindgen_anon_2.addr,
        ;;
        flags: types::TimeoutFlags = types::TimeoutFlags::empty() => sqe.__bindgen_anon_3.timeout_flags,
    }

    /// This request must be linked with another request through
    /// [`Flags::IO_LINK`](crate::squeue::Flags::IO_LINK) which is described below.
    /// Unlike [`Timeout`], [`LinkTimeout`] acts on the linked request, not the completion queue.
    pub struct LinkTimeoutSqe { }

    pub const CODE = sys::IORING_OP_LINK_TIMEOUT;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: {
            sqe.fd = -1;
            sqe.len = 1;
        }
        op_sqe: {
            self.sqe.fd = -1;
            self.sqe.fd = 1;
        }
    }
);

opcode!(
    /// Connect a socket, equivalent to `connect(2)`.
    pub struct Connect {
        fd: { impl sealed::UseFixed } => sqe.fd,
        addr: { *const libc::sockaddr } => sqe.__bindgen_anon_2.addr,
        addrlen: { libc::socklen_t } => sqe.__bindgen_anon_1.off,
        ;;
    }

    /// Connect a socket, equivalent to `connect(2)`.
    pub struct ConnectSqe { }

    pub const CODE = sys::IORING_OP_CONNECT;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { set_fd_flags!(sqe, self.fd); }
        op_sqe: { set_fd_flags!(self.sqe, fd); }
    }
);

// === 5.6 ===

opcode!(
    /// Preallocate or deallocate space to a file, equivalent to `fallocate(2)`.
    pub struct Fallocate {
        fd: { impl sealed::UseFixed } => sqe.fd,
        len: { libc::off_t } => sqe.__bindgen_anon_2.addr,
        ;;
        offset: libc::off_t = 0i64 => sqe.__bindgen_anon_1.off,
        mode: i32 = 0i32 => sqe.len,
    }

    /// Preallocate or deallocate space to a file, equivalent to `fallocate(2)`.
    pub struct FallocateSqe { }

    pub const CODE = sys::IORING_OP_FALLOCATE;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { set_fd_flags!(sqe, self.fd); }
        op_sqe: { set_fd_flags!(self.sqe, fd); }
    }
);

opcode!(
    /// Open a file, equivalent to `openat(2)`.
    pub struct OpenAt {
        dirfd: { impl sealed::UseFd } => sqe.fd,
        pathname: { *const libc::c_char } => sqe.__bindgen_anon_2.addr,
        ;;
        flags: i32 = 0i32 => sqe.__bindgen_anon_3.open_flags,
        mode: libc::mode_t = 0u32 => sqe.len,
    }

    /// Open a file, equivalent to `openat(2)`.
    pub struct OpenAtSqe { }

    pub const CODE = sys::IORING_OP_OPENAT;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) { }
);

opcode!(
    /// Close a file descriptor, equivalent to `close(2)`.
    pub struct Close {
        fd: { impl sealed::UseFd } => sqe.fd,
        ;;
    }

    /// Close a file descriptor, equivalent to `close(2)`.
    pub struct CloseSqe { }

    pub const CODE = sys::IORING_OP_CLOSE;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) { }
);

opcode!(
    /// This command is an alternative to using
    /// [`Submitter::register_files_update`](crate::Submitter::register_files_update) which then
    /// works in an async fashion, like the rest of the io_uring commands.
    pub struct FilesUpdate {
        fds: { *const RawFd } => sqe.__bindgen_anon_2.addr,
        len: { u32 } => sqe.len,
        ;;
        offset: i32 = 0i32 => sqe.__bindgen_anon_1.off,
    }

    /// This command is an alternative to using
    /// [`Submitter::register_files_update`](crate::Submitter::register_files_update) which then
    /// works in an async fashion, like the rest of the io_uring commands.
    pub struct FilesUpdateSqe { }

    pub const CODE = sys::IORING_OP_FILES_UPDATE;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { sqe.fd = -1; }
        op_sqe: { self.sqe.fd = -1; }
    }
);

opcode!(
    /// Get file status, equivalent to `statx(2)`.
    pub struct Statx {
        dirfd: { impl sealed::UseFd } => sqe.fd,
        pathname: { *const libc::c_char } => sqe.__bindgen_anon_2.addr,
        statxbuf: { *mut types::statx } => sqe.__bindgen_anon_1.off,
        ;;
        flags: i32 = 0i32 => sqe.__bindgen_anon_3.statx_flags,
        mask: u32 = 0u32 => sqe.len,
    }

    /// Get file status, equivalent to `statx(2)`.
    pub struct StatxSqe { }

    pub const CODE = sys::IORING_OP_STATX;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) { }
);

opcode!(
    /// Issue the equivalent of a `pread(2)` or `pwrite(2)` system call
    ///
    /// * `fd` is the file descriptor to be operated on,
    /// * `addr` contains the buffer in question,
    /// * `len` contains the length of the IO operation,
    ///
    /// These are non-vectored versions of the `IORING_OP_READV` and `IORING_OP_WRITEV` opcodes.
    /// See also `read(2)` and `write(2)` for the general description of the related system call.
    ///
    /// Available since 5.6.
    pub struct Read {
        fd: { impl sealed::UseFixed } => sqe.fd,
        buf: { *mut u8 } => sqe.__bindgen_anon_2.addr,
        len: { u32 } => sqe.len,
        ;;
        /// `offset` contains the read or write offset.
        ///
        /// If `fd` does not refer to a seekable file, `offset` must be set to zero.
        /// If `offsett` is set to `-1`, the offset will use (and advance) the file position,
        /// like the `read(2)` and `write(2)` system calls.
        offset: libc::off_t = 0i64 => sqe.__bindgen_anon_1.off,
        ioprio: u16 = 0u16 => sqe.ioprio,
        rw_flags: types::RwFlags = 0i32 => sqe.__bindgen_anon_3.rw_flags,
        buf_group: u16 = 0u16 => sqe.__bindgen_anon_4.buf_group,
    }

    /// Issue the equivalent of a `pread(2)` or `pwrite(2)` system call
    ///
    /// * `fd` is the file descriptor to be operated on,
    /// * `addr` contains the buffer in question,
    /// * `len` contains the length of the IO operation,
    ///
    /// These are non-vectored versions of the `IORING_OP_READV` and `IORING_OP_WRITEV` opcodes.
    /// See also `read(2)` and `write(2)` for the general description of the related system call.
    ///
    /// Available since 5.6.
    pub struct ReadSqe { }

    pub const CODE = sys::IORING_OP_READ;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { set_fd_flags!(sqe, self.fd); }
        op_sqe: { set_fd_flags!(self.sqe, fd); }
    }
);

opcode!(
    /// Issue the equivalent of a `pread(2)` or `pwrite(2)` system call
    ///
    /// * `fd` is the file descriptor to be operated on,
    /// * `addr` contains the buffer in question,
    /// * `len` contains the length of the IO operation,
    ///
    /// These are non-vectored versions of the `IORING_OP_READV` and `IORING_OP_WRITEV` opcodes.
    /// See also `read(2)` and `write(2)` for the general description of the related system call.
    ///
    /// Available since 5.6.
    pub struct Write {
        fd: { impl sealed::UseFixed } => sqe.fd,
        buf: { *const u8 } => sqe.__bindgen_anon_2.addr,
        len: { u32 } => sqe.len,
        ;;
        /// `offset` contains the read or write offset.
        ///
        /// If `fd` does not refer to a seekable file, `offset` must be set to zero.
        /// If `offsett` is set to `-1`, the offset will use (and advance) the file position,
        /// like the `read(2)` and `write(2)` system calls.
        offset: libc::off_t = 0i64 => sqe.__bindgen_anon_1.off,
        ioprio: u16 = 0u16 => sqe.ioprio,
        rw_flags: types::RwFlags = 0i32 => sqe.__bindgen_anon_3.rw_flags,
    }

    /// Issue the equivalent of a `pread(2)` or `pwrite(2)` system call
    ///
    /// * `fd` is the file descriptor to be operated on,
    /// * `addr` contains the buffer in question,
    /// * `len` contains the length of the IO operation,
    ///
    /// These are non-vectored versions of the `IORING_OP_READV` and `IORING_OP_WRITEV` opcodes.
    /// See also `read(2)` and `write(2)` for the general description of the related system call.
    ///
    /// Available since 5.6.
    pub struct WriteSqe { }

    pub const CODE = sys::IORING_OP_WRITE;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { set_fd_flags!(sqe, self.fd); }
        op_sqe: { set_fd_flags!(self.sqe, fd); }
    }
);

opcode!(
    /// Predeclare an access pattern for file data, equivalent to `posix_fadvise(2)`.
    pub struct Fadvise {
        fd: { impl sealed::UseFixed } => sqe.fd,
        len: { libc::off_t } => sqe.len,
        advice: { i32 } => sqe.__bindgen_anon_3.fadvise_advice,
        ;;
        offset: libc::off_t = 0i64 => sqe.__bindgen_anon_1.off,
    }

    /// Predeclare an access pattern for file data, equivalent to `posix_fadvise(2)`.
    pub struct FadviseSqe { }

    pub const CODE = sys::IORING_OP_FADVISE;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { set_fd_flags!(sqe, self.fd); }
        op_sqe: { set_fd_flags!(self.sqe, fd); }
    }
);

opcode!(
    /// Give advice about use of memory, equivalent to `madvise(2)`.
    pub struct Madvise {
        addr: { *const libc::c_void } => sqe.__bindgen_anon_2.addr,
        len: { libc::off_t } => sqe.len,
        advice: { i32 } => sqe.__bindgen_anon_3.fadvise_advice,
        ;;
    }

    /// Give advice about use of memory, equivalent to `madvise(2)`.
    pub struct MadviseSqe { }

    pub const CODE = sys::IORING_OP_MADVISE;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { sqe.fd = -1; }
        op_sqe: { self.sqe.fd = -1; }
    }
);

opcode!(
    /// Send a message on a socket, equivalent to `send(2)`.
    pub struct Send {
        fd: { impl sealed::UseFixed } => sqe.fd,
        buf: { *const u8 } => sqe.__bindgen_anon_2.addr,
        len: { u32 } => sqe.len,
        ;;
        flags: i32 = 0i32 => sqe.__bindgen_anon_3.msg_flags,
    }

    /// Send a message on a socket, equivalent to `send(2)`.
    pub struct SendSqe { }

    pub const CODE = sys::IORING_OP_SEND;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { set_fd_flags!(sqe, self.fd); }
        op_sqe: { set_fd_flags!(self.sqe, fd); }
    }
);

opcode!(
    /// Receive a message from a socket, equivalent to `recv(2)`.
    pub struct Recv {
        fd: { impl sealed::UseFixed } => sqe.fd,
        buf: { *mut u8 } => sqe.__bindgen_anon_2.addr,
        len: { u32 } => sqe.len,
        ;;
        flags: i32 = 0i32 => sqe.__bindgen_anon_3.msg_flags,
        buf_group: u16 = 0u16 => sqe.__bindgen_anon_4.buf_group,
    }

    /// Receive a message from a socket, equivalent to `recv(2)`.
    pub struct RecvSqe { }

    pub const CODE = sys::IORING_OP_RECV;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { set_fd_flags!(sqe, self.fd); }
        op_sqe: { set_fd_flags!(self.sqe, fd); }
    }
);

opcode!(
    /// Open a file, equivalent to `openat2(2)`.
    pub struct OpenAt2 {
        dirfd: { impl sealed::UseFd } => sqe.fd,
        pathname: { *const libc::c_char } => sqe.__bindgen_anon_2.addr,
        how: { *const types::OpenHow } => sqe.__bindgen_anon_1.off,
        ;;
    }

    /// Open a file, equivalent to `openat2(2)`.
    pub struct OpenAt2Sqe { }

    pub const CODE = sys::IORING_OP_OPENAT2;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { sqe.len = mem::size_of::<sys::open_how>() as _; }
        op_sqe: { self.sqe.len = mem::size_of::<sys::open_how>() as _;}
    }
);

opcode!(
    /// Modify an epoll file descriptor, equivalent to `epoll_ctl(2)`.
    pub struct EpollCtl {
        epfd: { impl sealed::UseFixed } => sqe.fd,
        fd: { impl sealed::UseFd } => sqe.__bindgen_anon_1.off,
        op: { i32 } => sqe.len,
        ev: { *const types::epoll_event } => sqe.__bindgen_anon_2.addr,
        ;;
    }

    /// Modify an epoll file descriptor, equivalent to `epoll_ctl(2)`.
    pub struct EpollCtlSqe { }

    pub const CODE = sys::IORING_OP_EPOLL_CTL;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { set_fd_flags!(sqe, self.epfd); }
        op_sqe: { set_fd_flags!(self.sqe, epfd); }
    }
);

// === 5.7 ===

opcode!(
    /// Splice data to/from a pipe, equivalent to `splice(2)`.
    ///
    /// if `fd_in` refers to a pipe, `off_in` must be `-1`;
    /// The description of `off_in` also applied to `off_out`.
    pub struct Splice {
        fd_in: { impl sealed::UseFixed } => sqe.__bindgen_anon_5.splice_fd_in,
        off_in: { i64 } => sqe.__bindgen_anon_2.splice_off_in,
        fd_out: { impl sealed::UseFixed } => sqe.fd,
        off_out: { i64 } => sqe.__bindgen_anon_1.off,
        len: { u32 } => sqe.len,
        ;;
        /// see man `splice(2)` for description of flags.
        flags: u32 = 0u32 => sqe.__bindgen_anon_3.splice_flags,
    }

    /// Splice data to/from a pipe, equivalent to `splice(2)`.
    ///
    /// if `fd_in` refers to a pipe, `off_in` must be `-1`;
    /// The description of `off_in` also applied to `off_out`.
    pub struct SpliceSqe { }

    pub const CODE = sys::IORING_OP_SPLICE;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: {
            set_fd_flags!(sqe, self.fd_out);
            if matches!(self.fd_in, sealed::Target::Fixed(_)) {
                sqe.__bindgen_anon_3.splice_flags = self.flags | sys::SPLICE_F_FD_IN_FIXED;
            }
        }
        op_sqe: {
            set_fd_flags!(self.sqe, fd_out);
            if matches!(fd_in, sealed::Target::Fixed(_)) {
                unsafe {
                    self.sqe.__bindgen_anon_3.splice_flags |= sys::SPLICE_F_FD_IN_FIXED;
                }
            }
        }
    }
);

#[cfg(feature = "unstable")]
opcode!(
    /// Register `nbufs` buffers that each have the length `len` with ids starting from `big` in the
    /// group `bgid` that can be used for any request. See
    /// [`BUFFER_SELECT`](crate::squeue::Flags::BUFFER_SELECT) for more info.
    ///
    /// Requires the `unstable` feature.
    pub struct ProvideBuffers {
        addr: { *mut u8 } => sqe.__bindgen_anon_2.addr,
        len: { i32 } => sqe.len,
        nbufs: { u16 } => sqe.fd,
        bgid: { u16 } => sqe.__bindgen_anon_4.buf_group,
        bid: { u16 } => sqe.__bindgen_anon_1.off,
        ;;
    }

    /// Register `nbufs` buffers that each have the length `len` with ids starting from `big` in the
    /// group `bgid` that can be used for any request. See
    /// [`BUFFER_SELECT`](crate::squeue::Flags::BUFFER_SELECT) for more info.
    ///
    /// Requires the `unstable` feature.
    pub struct ProvideBuffersSqe { }

    pub const CODE = sys::IORING_OP_PROVIDE_BUFFERS;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) { }
);

#[cfg(feature = "unstable")]
opcode!(
    /// Remove some number of buffers from a buffer group. See
    /// [`BUFFER_SELECT`](crate::squeue::Flags::BUFFER_SELECT) for more info.
    ///
    /// Requires the `unstable` feature.
    pub struct RemoveBuffers {
        nbufs: { u16 } => sqe.fd,
        bgid: { u16 } => sqe.__bindgen_anon_4.buf_group,
        ;;
    }

    /// Remove some number of buffers from a buffer group. See
    /// [`BUFFER_SELECT`](crate::squeue::Flags::BUFFER_SELECT) for more info.
    ///
    /// Requires the `unstable` feature.
    pub struct RemoveBuffersSqe { }

    pub const CODE = sys::IORING_OP_REMOVE_BUFFERS;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) { }
);

// === 5.8 ===

#[cfg(feature = "unstable")]
opcode!(
    /// Duplicate pipe content, equivalent to `tee(2)`.
    ///
    /// Requires the `unstable` feature.
    pub struct Tee {
        fd_in: { impl sealed::UseFixed } => sqe.__bindgen_anon_5.splice_fd_in,
        fd_out: { impl sealed::UseFixed } => sqe.fd,
        len: { u32 } => sqe.len,
        ;;
        flags: u32 = 0 => sqe.__bindgen_anon_3.splice_flags,
    }

    /// Duplicate pipe content, equivalent to `tee(2)`.
    ///
    /// Requires the `unstable` feature.
    pub struct TeeSqe { }

    pub const CODE = sys::IORING_OP_TEE;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: {
            set_fd_flags!(sqe, self.fd_out);
            if matches!(self.fd_in, sealed::Target::Fixed(_)) {
                sqe.__bindgen_anon_3.splice_flags = self.flags | sys::SPLICE_F_FD_IN_FIXED;
            }
        }
        op_sqe: {
            set_fd_flags!(self.sqe, fd_out);
            if matches!(fd_in, sealed::Target::Fixed(_)) {
                unsafe {
                    self.sqe.__bindgen_anon_3.splice_flags |= sys::SPLICE_F_FD_IN_FIXED;
                }
            }
        }
    }
);

// === 5.11 ===

#[cfg(feature = "unstable")]
opcode!(
    pub struct Shutdown {
        fd: { impl sealed::UseFixed } => sqe.fd,
        how: { i32 } => sqe.len,
        ;;
    }

    pub struct ShutdownSqe { }

    pub const CODE = sys::IORING_OP_SHUTDOWN;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) {
        op: { set_fd_flags!(sqe, self.fd); }
        op_sqe: { set_fd_flags!(self.sqe, fd); }
    }
);

#[cfg(feature = "unstable")]
opcode!(
    pub struct RenameAt {
        olddirfd: { impl sealed::UseFd } => sqe.fd,
        oldpath: { *const libc::c_char } => sqe.__bindgen_anon_2.addr,
        newdirfd: { impl sealed::UseFd } => sqe.len,
        newpath: { *const libc::c_char } => sqe.__bindgen_anon_1.off,
        ;;
        flags: u32 = 0u32 => sqe.__bindgen_anon_3.rename_flags,
    }

    pub struct RenameAtSqe { }

    pub const CODE = sys::IORING_OP_RENAMEAT;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) { }
);

#[cfg(feature = "unstable")]
opcode!(
    pub struct UnlinkAt {
        dirfd: { impl sealed::UseFd } => sqe.fd,
        pathname: { *const libc::c_char } => sqe.__bindgen_anon_2.addr,
        ;;
        flags: i32 = 0i32 => sqe.__bindgen_anon_3.unlink_flags,
    }

    pub struct UnlinkAtSqe { }

    pub const CODE = sys::IORING_OP_UNLINKAT;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) { }
);

// === 5.15 ===

#[cfg(feature = "unstable")]
opcode!(
    /// Make a directory, equivalent to `mkdirat2(2)`.
    ///
    /// Requires the `unstable` feature.
    pub struct MkDirAt {
        dirfd: { impl sealed::UseFd } => sqe.fd,
        pathname: { *const libc::c_char } => sqe.__bindgen_anon_2.addr,
        ;;
        mode: libc::mode_t = 0u32 => sqe.len,
    }

    pub struct MkDirAtSqe { }

    pub const CODE = sys::IORING_OP_MKDIRAT;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) { }
);

#[cfg(feature = "unstable")]
opcode!(
    /// Create a symlink, equivalent to `symlinkat2(2)`.
    ///
    /// Requires the `unstable` feature.
    pub struct SymlinkAt {
        newdirfd: { impl sealed::UseFd } => sqe.fd,
        target: { *const libc::c_char } => sqe.__bindgen_anon_2.addr,
        linkpath: { *const libc::c_char } => sqe.__bindgen_anon_1.addr2,
        ;;
    }

    /// Create a symlink, equivalent to `symlinkat2(2)`.
    ///
    /// Requires the `unstable` feature.
    pub struct SymlinkAtSqe { }

    pub const CODE = sys::IORING_OP_SYMLINKAT;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) { }
);

#[cfg(feature = "unstable")]
opcode!(
    /// Create a hard link, equivalent to `linkat2(2)`.
    ///
    /// Requires the `unstable` feature.
    pub struct LinkAt {
        olddirfd: { impl sealed::UseFd } => sqe.fd,
        oldpath: { *const libc::c_char } => sqe.__bindgen_anon_2.addr,
        newdirfd: { impl sealed::UseFd } => sqe.len,
        newpath: { *const libc::c_char } => sqe.__bindgen_anon_1.addr2 ,
        ;;
        flags: i32 = 0i32 => sqe.__bindgen_anon_3.hardlink_flags,
    }

    /// Create a hard link, equivalent to `linkat2(2)`.
    ///
    /// Requires the `unstable` feature.
    pub struct LinkAtSqe { }

    pub const CODE = sys::IORING_OP_LINKAT;

    fn set_special_fields(&mut self, sqe: &mut sys::io_uring_sqe) { }
);
