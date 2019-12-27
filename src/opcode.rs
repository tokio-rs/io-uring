//! Operation code

#![allow(clippy::new_without_default)]

use linux_io_uring_sys as sys;
use crate::squeue::Entry;
use crate::util::sqe_zeroed;

pub mod types {
    use std::os::unix::io::RawFd;
    use bitflags::bitflags;
    use linux_io_uring_sys as sys;

    pub use sys::__kernel_timespec as Timespec;
    pub use sys::__kernel_rwf_t as RwFlags;

    #[derive(Debug, Clone, Copy)]
    pub enum Target {
        Fd(RawFd),

        /// The index of registered fd.
        Fixed(u32)
    }

    bitflags!{
        pub struct TimeoutFlags: u32 {
            #[cfg(feature = "unstable")]
            const ABS = sys::IORING_TIMEOUT_ABS;
        }
    }

    bitflags!{
        pub struct FsyncFlags: u32 {
            const DATASYNC = sys::IORING_FSYNC_DATASYNC;
        }
    }
}


macro_rules! assign_fd {
    ( $sqe:ident . fd = $opfd:expr ) => {
        match $opfd {
            types::Target::Fd(fd) => $sqe.fd = fd,
            types::Target::Fixed(i) => {
                $sqe.fd = i as _;
                $sqe.flags |= crate::squeue::Flags::FIXED_FILE.bits();
            }
        }
    }
}

macro_rules! opcode {
    (
        $( #[$outer:meta] )*
        pub struct $name:ident {
            $( #[$new_meta:meta] )*
            $( $field:ident : $tname:ty ),* $(,)?
            ;;
            $(
                $( #[$opt_meta:meta] )*
                $opt_field:ident : $opt_tname:ty = $default:expr
            ),* $(,)?
        }

        pub fn build($self:ident) -> Entry $build_block:block
    ) => {
        $( #[$outer] )*
        pub struct $name {
            $( $field : $tname, )*
            $( $opt_field : $opt_tname, )*
        }

        impl $name {
            $( #[$new_meta] )*
            pub const fn new( $( $field : $tname ),* ) -> Self {
                $name {
                    $( $field , )*
                    $( $opt_field: $default, )*
                }
            }

            $(
                $( #[$opt_meta] )*
                pub const fn $opt_field(mut self, $opt_field: $opt_tname) -> Self {
                    self.$opt_field = $opt_field;
                    self
                }
            )*

            pub fn build($self) -> Entry $build_block
        }
    }
}

opcode!(
    /// Do not perform any I/O.
    ///
    /// This is useful for testing the performance of the io_uring implementation itself.
    #[derive(Debug)]
    pub struct Nop { ;; }

    pub fn build(self) -> Entry {
        let Nop {} = self;

        let mut sqe = sqe_zeroed();
        sqe.opcode = sys::IORING_OP_NOP as _;
        sqe.fd = -1;
        Entry(sqe)
    }
);

opcode!(
    /// Vectored read operations, similar to `preadv2 (2)`.
    ///
    /// The return values match those documented in the `preadv2 (2)` man pages.
    #[derive(Debug)]
    pub struct Readv {
        fd: types::Target,
        iovec: *mut libc::iovec,
        len: u32,
        ;;
        ioprio: u16 = 0,
        offset: libc::off_t = 0,
        /// specified for read operations, contains a bitwise OR of per-I/O flags,
        /// as described in the `preadv2 (2)` man page.
        rw_flags: types::RwFlags = 0
    }

    pub fn build(self) -> Entry {
        let Readv {
            fd,
            iovec, len, offset,
            ioprio, rw_flags
        } = self;

        let mut sqe = sqe_zeroed();
        sqe.opcode = sys::IORING_OP_READV as _;
        assign_fd!(sqe.fd = fd);
        sqe.ioprio = ioprio;
        sqe.addr = iovec as _;
        sqe.len = len;
        sqe.__bindgen_anon_1.off = offset as _;
        sqe.__bindgen_anon_2.rw_flags = rw_flags;
        Entry(sqe)
    }
);

opcode!(
    /// Vectored write operations, similar to `pwritev2 (2)`.
    ///
    /// The return values match those documented in the `pwritev2 (2)` man pages.
    #[derive(Debug)]
    pub struct Writev {
        fd: types::Target,
        iovec: *const libc::iovec,
        len: u32,
        ;;
        ioprio: u16 = 0,
        offset: libc::off_t = 0,
        /// specified for write operations, contains a bitwise OR of per-I/O flags,
        /// as described in the `preadv2 (2)` man page.
        rw_flags: types::RwFlags = 0
    }

    pub fn build(self) -> Entry {
        let Writev {
            fd,
            iovec, len, offset,
            ioprio, rw_flags
        } = self;

        let mut sqe = sqe_zeroed();
        sqe.opcode = sys::IORING_OP_WRITEV as _;
        assign_fd!(sqe.fd = fd);
        sqe.ioprio = ioprio;
        sqe.addr = iovec as _;
        sqe.len = len;
        sqe.__bindgen_anon_1.off = offset as _;
        sqe.__bindgen_anon_2.rw_flags = rw_flags;
        Entry(sqe)
    }
);

opcode!(
    /// File sync. See also `fsync (2)`.
    ///
    /// Note that, while I/O is initiated in the order in which it appears in the submission queue, completions are unordered.
    /// For example, an application which places a write I/O followed by an fsync in the submission queue cannot expect the fsync to apply to the write. The two operations execute in parallel, so the fsync may complete before the write is issued to the storage. The same is also true for previously issued writes that have not completed prior to the fsync.
    #[derive(Debug)]
    pub struct Fsync {
        fd: types::Target,
        ;;
        /// The `flags` bit mask may contain either 0, for a normal file integrity sync,
        /// or [types::FsyncFlags::DATASYNC] to provide data sync only semantics.
        /// See the descriptions of `O_SYNC` and `O_DSYNC` in the `open (2)` manual page for more information.
        flags: types::FsyncFlags = types::FsyncFlags::empty()
    }

    pub fn build(self) -> Entry {
        let Fsync { fd, flags } = self;

        let mut sqe = sqe_zeroed();
        sqe.opcode = sys::IORING_OP_FSYNC as _;
        assign_fd!(sqe.fd = fd);
        sqe.__bindgen_anon_2.fsync_flags = flags.bits();
        Entry(sqe)
    }
);

opcode!(
    /// Read from pre-mapped buffers.
    ///
    /// The return values match those documented in the `preadv2 (2)` man pages.
    #[derive(Debug)]
    pub struct ReadFixed {
        /// The `buf_index` is an index into an array of fixed buffers,
        /// and is only valid if fixed buffers were registered.
        fd: types::Target,
        buf: *mut u8,
        len: u32,
        buf_index: u16,
        ;;
        ioprio: u16 = 0,
        offset: libc::off_t = 0,
        /// specified for read operations, contains a bitwise OR of per-I/O flags,
        /// as described in the `preadv2 (2)` man page.
        rw_flags: types::RwFlags = 0
    }

    pub fn build(self) -> Entry {
        let ReadFixed {
            fd,
            buf, len, offset,
            buf_index,
            ioprio, rw_flags
        } = self;

        let mut sqe = sqe_zeroed();
        sqe.opcode = sys::IORING_OP_READ_FIXED as _;
        assign_fd!(sqe.fd = fd);
        sqe.ioprio = ioprio;
        sqe.addr = buf as _;
        sqe.len = len;
        sqe.__bindgen_anon_1.off = offset as _;
        sqe.__bindgen_anon_2.rw_flags = rw_flags;
        sqe.__bindgen_anon_3.buf_index = buf_index;
        Entry(sqe)
    }
);

opcode!(
    /// Write to pre-mapped buffers.
    ///
    /// The return values match those documented in the `pwritev2 (2)` man pages.
    #[derive(Debug)]
    pub struct WriteFixed {
        /// The `buf_index` is an index into an array of fixed buffers,
        /// and is only valid if fixed buffers were registered.
        fd: types::Target,
        buf: *const u8,
        len: u32,
        buf_index: u16,
        ;;
        ioprio: u16 = 0,
        offset: libc::off_t = 0,
        /// specified for write operations, contains a bitwise OR of per-I/O flags,
        /// as described in the `preadv2 (2)` man page.
        rw_flags: types::RwFlags = 0
    }

    pub fn build(self) -> Entry {
        let WriteFixed {
            fd,
            buf, len, offset,
            buf_index,
            ioprio, rw_flags
        } = self;

        let mut sqe = sqe_zeroed();
        sqe.opcode = sys::IORING_OP_WRITE_FIXED as _;
        assign_fd!(sqe.fd = fd);
        sqe.ioprio = ioprio;
        sqe.addr = buf as _;
        sqe.len = len;
        sqe.__bindgen_anon_1.off = offset as _;
        sqe.__bindgen_anon_2.rw_flags = rw_flags;
        sqe.__bindgen_anon_3.buf_index = buf_index;
        Entry(sqe)
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
        /// and documented in `poll (2)`.
        fd: types::Target,
        flags: libc::c_short,
        ;;
    }

    pub fn build(self) -> Entry {
        let PollAdd { fd, flags } = self;

        let mut sqe = sqe_zeroed();
        sqe.opcode = sys::IORING_OP_POLL_ADD as _;
        assign_fd!(sqe.fd = fd);
        sqe.__bindgen_anon_2.poll_events = flags as _;
        Entry(sqe)
    }
);

opcode!(
    /// Remove an existing poll request.
    ///
    /// If found, the `result` method of the `cqueue::Entry` will return 0.
    /// If not found, `result` will return `-libc::ENOENT`.
    #[derive(Debug)]
    pub struct PollRemove {
        user_data: u64
        ;;
    }

    pub fn build(self) -> Entry {
        let PollRemove { user_data } = self;

        let mut sqe = sqe_zeroed();
        sqe.opcode = sys::IORING_OP_POLL_REMOVE as _;
        sqe.fd = -1;
        sqe.addr = user_data as _;
        Entry(sqe)
    }
);

opcode!(
    /// Issue the equivalent of a `sync_file_range (2)` on the file descriptor.
    ///
    /// See also `sync_file_range (2)`. for the general description of the related system call.
    #[derive(Debug)]
    pub struct SyncFileRange {
        fd: types::Target,
        len: u32,
        ;;
        /// the offset method holds the offset in bytes
        offset: libc::off_t = 0,
        /// the flags method holds the flags for the command
        flags: u32 = 0
    }

    pub fn build(self) -> Entry {
        let SyncFileRange {
            fd,
            len, offset,
            flags
        } = self;

        let mut sqe = sqe_zeroed();
        sqe.opcode = sys::IORING_OP_SYNC_FILE_RANGE as _;
        assign_fd!(sqe.fd = fd);
        sqe.len = len;
        sqe.__bindgen_anon_1.off = offset as _;
        sqe.__bindgen_anon_2.sync_range_flags = flags;
        Entry(sqe)
    }
);

opcode!(
    /// Issue the equivalent of a `sendmsg (2)` system call.
    ///
    /// fd must be set to the socket file descriptor, addr must contains a pointer to the msghdr structure,
    /// and flags holds the flags associated with the system call.
    /// See also `sendmsg (2)`. for the general description of the related system call.
    #[derive(Debug)]
    pub struct SendMsg {
        fd: types::Target,
        msg: *const libc::msghdr,
        ;;
        ioprio: u16 = 0,
        flags: u32 = 0
    }

    pub fn build(self) -> Entry {
        let SendMsg { fd, msg, ioprio, flags } = self;

        let mut sqe = sqe_zeroed();
        sqe.opcode = sys::IORING_OP_SENDMSG as _;
        assign_fd!(sqe.fd = fd);
        sqe.ioprio = ioprio;
        sqe.addr = msg as _;
        sqe.len = 1;
        sqe.__bindgen_anon_2.msg_flags = flags;
        Entry(sqe)
    }
);

opcode!(
    /// Works just like [SendMsg], except for instead.
    ///
    /// See the description of [SendMsg].
    #[derive(Debug)]
    pub struct RecvMsg {
        fd: types::Target,
        msg: *mut libc::msghdr,
        ;;
        ioprio: u16 = 0,
        flags: u32 = 0
    }

    pub fn build(self) -> Entry {
        let RecvMsg { fd, msg, ioprio, flags } = self;

        let mut sqe = sqe_zeroed();
        sqe.opcode = sys::IORING_OP_RECVMSG as _;
        assign_fd!(sqe.fd = fd);
        sqe.ioprio = ioprio;
        sqe.addr = msg as _;
        sqe.len = 1;
        sqe.__bindgen_anon_2.msg_flags = flags;
        Entry(sqe)
    }
);

opcode!(
    /// This command will register a timeout operation.
    ///
    /// A timeout will trigger a wakeup event on the completion ring for anyone waiting for events.
    /// A timeout condition is met when either the specified timeout expires, or the specified number of events have completed.
    /// Either condition will trigger the event.
    /// The request will complete with `-ETIME` if the timeout got completed through expiration of the timer,
    /// or 0 if the timeout got completed through requests completing on their own.
    /// If the timeout was cancelled before it expired, the request will complete with `-ECANCELED`.
    #[derive(Debug)]
    pub struct Timeout {
        timespec: *const types::Timespec,
        ;;
        /// `count` may contain a completion event count. If not set, this defaults to 1.
        count: u32 = 0,

        #[cfg_attr(not(feature = "unstable"), allow(intra_doc_link_resolution_failure))]
        /// `flags` may contain [types::TimeoutFlags::ABS] for an absolutel timeout value, or 0 for a relative timeout.
        flags: types::TimeoutFlags = types::TimeoutFlags::empty()
    }

    pub fn build(self) -> Entry {
        let Timeout { timespec, count, flags } = self;

        let mut sqe = sqe_zeroed();
        sqe.opcode = sys::IORING_OP_TIMEOUT as _;
        sqe.fd = -1;
        sqe.addr = timespec as _;
        sqe.len = 1;
        sqe.__bindgen_anon_1.off = count as _;
        sqe.__bindgen_anon_2.timeout_flags = flags.bits();
        Entry(sqe)
    }
);

#[cfg(feature = "unstable")]
opcode!(
    pub struct TimeoutRemove {
        user_data: u64,
        ;;
        flags: types::TimeoutFlags = types::TimeoutFlags::empty()
    }

    pub fn build(self) -> Entry {
        let TimeoutRemove { user_data, flags } = self;

        let mut sqe = sqe_zeroed();
        sqe.opcode = sys::IORING_OP_TIMEOUT_REMOVE as _;
        sqe.fd = -1;
        sqe.addr = user_data as _;
        sqe.__bindgen_anon_2.timeout_flags = flags.bits();
        Entry(sqe)
    }
);

#[cfg(feature = "unstable")]
opcode!(
    pub struct Accept {
        fd: types::Target,
        addr: *const libc::sockaddr,
        addrlen: libc::socklen_t
        ;;
        flags: u32 = 0
    }

    pub fn build(self) -> Entry {
        let Accept { fd, addr, addrlen, flags } = self;

        let mut sqe = sqe_zeroed();
        sqe.opcode = sys::IORING_OP_ACCEPT as _;
        assign_fd!(sqe.fd = fd);
        sqe.addr = addr as _;
        sqe.__bindgen_anon_1.off = addrlen as _;
        sqe.__bindgen_anon_2.accept_flags = flags;
        Entry(sqe)
    }
);

#[cfg(feature = "unstable")]
opcode!(
    pub struct AsyncCancel {
        user_data: u64,
        ;;
        flags: u32 = 0
    }

    pub fn build(self) -> Entry {
        let AsyncCancel { user_data, flags } = self;

        let mut sqe = sqe_zeroed();
        sqe.opcode = sys::IORING_OP_ASYNC_CANCEL as _;
        sqe.fd = -1;
        sqe.addr = user_data as _;
        sqe.__bindgen_anon_2.timeout_flags = flags;
        Entry(sqe)
    }
);

#[cfg(feature = "unstable")]
opcode!(
    pub struct LinkTimeout {
        timespec: *const types::Timespec,
        ;;
        flags: types::TimeoutFlags = types::TimeoutFlags::empty()
    }

    pub fn build(self) -> Entry {
        let LinkTimeout { timespec, flags } = self;

        let mut sqe = sqe_zeroed();
        sqe.opcode = sys::IORING_OP_TIMEOUT as _;
        sqe.fd = -1;
        sqe.addr = timespec as _;
        sqe.len = 1;
        sqe.__bindgen_anon_2.timeout_flags = flags.bits();
        Entry(sqe)
    }
);

#[cfg(feature = "unstable")]
opcode!(
    pub struct Connect {
        fd: types::Target,
        addr: *const libc::sockaddr,
        addrlen: libc::socklen_t
        ;;
    }

    pub fn build(self) -> Entry {
        let Connect { fd, addr, addrlen } = self;

        let mut sqe = sqe_zeroed();
        sqe.opcode = sys::IORING_OP_CONNECT as _;
        assign_fd!(sqe.fd = fd);
        sqe.addr = addr as _;
        sqe.__bindgen_anon_1.off = addrlen as _;
        Entry(sqe)
    }
);
