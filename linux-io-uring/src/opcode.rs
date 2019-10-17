use std::os::unix::io::RawFd;
use linux_io_uring_sys as sys;
use crate::squeue::Entry;


pub struct Nop;

pub struct Readv {
    fd: RawFd,
    iovec: *mut libc::iovec,
    len: u32,
    offset: i64
}

pub struct Writev {
    fd: RawFd,
    iovec: *const libc::iovec,
    len: u32,
    offset: i64
}

pub struct Fsync {
    fd: RawFd,
    flags: u32
}

pub struct ReadFixed {
    fd: RawFd,
    buf: *mut u8,
    len: u32,
    offset: i64,
    buf_index: u16
}

pub struct WriteFixed {
    fd: RawFd,
    buf: *const u8,
    len: u32,
    offset: i64,
    buf_index: u16
}

pub struct PollAdd {
    fd: RawFd,
    mask: i16
}

pub struct PollRemove {
    user_data: *const libc::c_void
}

pub struct SyncFileRange {
    fd: RawFd,
    len: u32,
    offset: i64,
    flags: u32
}

pub struct SendMsg {
    fd: RawFd,
    msg: *const libc::msghdr,
    flags: u32
}

pub struct RecvMsg {
    fd: RawFd,
    msg: *mut libc::msghdr,
    flags: u32
}

impl From<Nop> for Entry {
    fn from(op: Nop) -> Entry {
        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_NOP as _;
        Entry(sqe)
    }
}

impl From<Readv> for Entry {
    fn from(op: Readv) -> Entry {
        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_READV as _;
        sqe.fd = op.fd;
        sqe.addr = op.iovec as _;
        sqe.len = op.len;
        sqe.off = op.offset as _;
        Entry(sqe)
    }
}

impl From<Writev> for Entry {
    fn from(op: Writev) -> Entry {
        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_WRITEV as _;
        sqe.fd = op.fd;
        sqe.addr = op.iovec as _;
        sqe.len = op.len;
        sqe.off = op.offset as _;
        Entry(sqe)
    }
}

impl From<Fsync> for Entry {
    fn from(op: Fsync) -> Entry {
        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_FSYNC as _;
        sqe.fd = op.fd;
        sqe.__bindgen_anon_1.fsync_flags = op.flags;
        Entry(sqe)
    }
}

impl From<ReadFixed> for Entry {
    fn from(op: ReadFixed) -> Entry {
        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_READ_FIXED as _;
        sqe.fd = op.fd;
        sqe.addr = op.buf as _;
        sqe.len = op.len;
        sqe.off = op.offset as _;
        sqe.__bindgen_anon_2.buf_index = op.buf_index;
        Entry(sqe)
    }
}

impl From<PollAdd> for Entry {
    fn from(op: PollAdd) -> Entry {
        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_POLL_ADD as _;
        sqe.fd = op.fd;
        sqe.__bindgen_anon_1.poll_events = op.mask as _;
        Entry(sqe)
    }
}

impl From<PollRemove> for Entry {
    fn from(op: PollRemove) -> Entry {
        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_POLL_REMOVE as _;
        sqe.addr = op.user_data as _;
        Entry(sqe)
    }
}

impl From<SyncFileRange> for Entry {
    fn from(op: SyncFileRange) -> Entry {
        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_SYNC_FILE_RANGE as _;
        sqe.len = op.len;
        sqe.off = op.offset as _;
        sqe.__bindgen_anon_1.sync_range_flags = op.flags;
        Entry(sqe)
    }
}

impl From<SendMsg> for Entry {
    fn from(op: SendMsg) -> Entry {
        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_SENDMSG as _;
        sqe.fd = op.fd;
        sqe.addr = op.msg as _;
        sqe.__bindgen_anon_1.msg_flags = op.flags;
        Entry(sqe)
    }
}

impl From<RecvMsg> for Entry {
    fn from(op: RecvMsg) -> Entry {
        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_RECVMSG as _;
        sqe.fd = op.fd;
        sqe.addr = op.msg as _;
        sqe.__bindgen_anon_1.msg_flags = op.flags;
        Entry(sqe)
    }
}
