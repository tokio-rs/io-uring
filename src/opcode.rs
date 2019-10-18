use std::ptr;
use std::marker::PhantomData;
use std::os::unix::io::RawFd;
use linux_io_uring_sys as sys;
use crate::squeue::Entry;


pub enum Target {
    Fd(RawFd),
    Fixed(u32)
}

#[derive(Default)]
pub struct Nop;

pub struct Readv {
    pub fd: Target,
    pub iovec: *mut libc::iovec,
    pub len: u32,
    pub offset: i64,
    pub rw_flags: i32,
    _mark: PhantomData<()>
}

pub struct Writev {
    pub fd: Target,
    pub iovec: *const libc::iovec,
    pub len: u32,
    pub offset: i64,
    pub rw_flags: i32,
    _mark: PhantomData<()>
}

pub struct Fsync {
    pub fd: Target,
    pub flags: u32,
    _mark: PhantomData<()>
}

pub struct ReadFixed {
    pub fd: Target,
    pub buf: *mut u8,
    pub len: u32,
    pub offset: i64,
    pub rw_flags: i32,
    pub buf_index: u16,
    _mark: PhantomData<()>
}

pub struct WriteFixed {
    pub fd: Target,
    pub buf: *const u8,
    pub len: u32,
    pub offset: i64,
    pub buf_index: u16,
    pub rw_flags: i32,
    _mark: PhantomData<()>
}

pub struct PollAdd {
    pub fd: Target,
    pub mask: u16,
    _mark: PhantomData<()>
}

pub struct PollRemove {
    pub user_data: *const libc::c_void,
    _mark: PhantomData<()>
}

pub struct SyncFileRange {
    pub fd: Target,
    pub len: u32,
    pub offset: i64,
    pub flags: u32,
    _mark: PhantomData<()>
}

pub struct SendMsg {
    pub fd: Target,
    pub msg: *const libc::msghdr,
    pub flags: u32,
    _mark: PhantomData<()>
}

pub struct RecvMsg {
    pub fd: Target,
    pub msg: *mut libc::msghdr,
    pub flags: u32,
    _mark: PhantomData<()>
}

// derive raw ptr
impl Default for Readv {
    fn default() -> Readv {
        Readv {
            fd: Target::Fd(-1),
            iovec: ptr::null_mut(),
            len: 0,
            offset: 0,
            rw_flags: 0,
            _mark: PhantomData
        }
    }
}

impl Default for Writev {
    fn default() -> Writev {
        Writev {
            fd: Target::Fd(-1),
            iovec: ptr::null(),
            len: 0,
            offset: 0,
            rw_flags: 0,
            _mark: PhantomData
        }
    }
}

impl Default for Fsync {
    fn default() -> Fsync {
        Fsync {
            fd: Target::Fd(-1),
            flags: 0,
            _mark: PhantomData
        }
    }
}

impl Default for ReadFixed {
    fn default() -> ReadFixed {
        ReadFixed {
            fd: Target::Fd(-1),
            buf: ptr::null_mut(),
            len: 0,
            offset: 0,
            rw_flags: 0,
            buf_index: 0,
            _mark: PhantomData
        }
    }
}

impl Default for WriteFixed {
    fn default() -> WriteFixed {
        WriteFixed {
            fd: Target::Fd(-1),
            buf: ptr::null(),
            len: 0,
            offset: 0,
            rw_flags: 0,
            buf_index: 0,
            _mark: PhantomData
        }
    }
}

impl Default for PollAdd {
    fn default() -> PollAdd {
        PollAdd {
            fd: Target::Fd(-1),
            mask: 0,
            _mark: PhantomData
        }
    }
}

impl Default for PollRemove {
    fn default() -> PollRemove {
        PollRemove {
            user_data: ptr::null(),
            _mark: PhantomData
        }
    }
}

impl Default for SyncFileRange {
    fn default() -> SyncFileRange {
        SyncFileRange {
            fd: Target::Fd(-1),
            len: 0,
            offset: 0,
            flags: 0,
            _mark: PhantomData
        }
    }
}

impl Default for SendMsg {
    fn default() -> SendMsg {
        SendMsg {
            fd: Target::Fd(-1),
            msg: ptr::null(),
            flags: 0,
            _mark: PhantomData
        }
    }
}

impl Default for RecvMsg {
    fn default() -> RecvMsg {
        RecvMsg {
            fd: Target::Fd(-1),
            msg: ptr::null_mut(),
            flags: 0,
            _mark: PhantomData
        }
    }
}

macro_rules! assign_fd {
    ( $sqe:ident . fd = $opfd:expr ) => {
        match $opfd {
            Target::Fd(fd) => $sqe.fd = fd,
            Target::Fixed(i) => {
                $sqe.fd = i as _;
                $sqe.flags |= crate::squeue::Flags::FIXED_FILE.bits();
            }
        }
    }
}

impl From<Nop> for Entry {
    fn from(op: Nop) -> Entry {
        let Nop = op;

        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_NOP as _;
        Entry(sqe)
    }
}

impl From<Readv> for Entry {
    fn from(op: Readv) -> Entry {
        let Readv {
            fd,
            iovec, len, offset,
            rw_flags,
            _mark
        } = op;

        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_READV as _;
        assign_fd!(sqe.fd = fd);
        sqe.addr = iovec as _;
        sqe.len = len;
        sqe.off = offset as _;
        sqe.__bindgen_anon_1.rw_flags = rw_flags;
        Entry(sqe)
    }
}

impl From<Writev> for Entry {
    fn from(op: Writev) -> Entry {
        let Writev {
            fd,
            iovec, len, offset,
            rw_flags,
            _mark
        } = op;

        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_WRITEV as _;
        assign_fd!(sqe.fd = fd);
        sqe.addr = iovec as _;
        sqe.len = len;
        sqe.off = offset as _;
        sqe.__bindgen_anon_1.rw_flags = rw_flags;
        Entry(sqe)
    }
}

impl From<Fsync> for Entry {
    fn from(op: Fsync) -> Entry {
        let Fsync { fd, flags, _mark } = op;

        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_FSYNC as _;
        assign_fd!(sqe.fd = fd);
        sqe.__bindgen_anon_1.fsync_flags = flags;
        Entry(sqe)
    }
}

impl From<ReadFixed> for Entry {
    fn from(op: ReadFixed) -> Entry {
        let ReadFixed {
            fd,
            buf, len, offset,
            rw_flags,
            buf_index,
            _mark
        } = op;

        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_READ_FIXED as _;
        assign_fd!(sqe.fd = fd);
        sqe.addr = buf as _;
        sqe.len = len;
        sqe.off = offset as _;
        sqe.__bindgen_anon_1.rw_flags = rw_flags;
        sqe.__bindgen_anon_2.buf_index = buf_index;
        Entry(sqe)
    }
}

impl From<WriteFixed> for Entry {
    fn from(op: WriteFixed) -> Entry {
        let WriteFixed {
            fd,
            buf, len, offset,
            rw_flags,
            buf_index,
            _mark
        } = op;

        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_WRITE_FIXED as _;
        assign_fd!(sqe.fd = fd);
        sqe.addr = buf as _;
        sqe.len = len;
        sqe.off = offset as _;
        sqe.__bindgen_anon_1.rw_flags = rw_flags;
        sqe.__bindgen_anon_2.buf_index = buf_index;
        Entry(sqe)
    }
}

impl From<PollAdd> for Entry {
    fn from(op: PollAdd) -> Entry {
        let PollAdd { fd, mask, _mark } = op;

        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_POLL_ADD as _;
        assign_fd!(sqe.fd = fd);
        sqe.__bindgen_anon_1.poll_events = mask as _;
        Entry(sqe)
    }
}

impl From<PollRemove> for Entry {
    fn from(op: PollRemove) -> Entry {
        let PollRemove { user_data, _mark } = op;

        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_POLL_REMOVE as _;
        sqe.addr = user_data as _;
        Entry(sqe)
    }
}

impl From<SyncFileRange> for Entry {
    fn from(op: SyncFileRange) -> Entry {
        let SyncFileRange {
            fd,
            len, offset,
            flags,
            _mark
        } = op;

        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_SYNC_FILE_RANGE as _;
        assign_fd!(sqe.fd = fd);
        sqe.len = len;
        sqe.off = offset as _;
        sqe.__bindgen_anon_1.sync_range_flags = flags;
        Entry(sqe)
    }
}

impl From<SendMsg> for Entry {
    fn from(op: SendMsg) -> Entry {
        let SendMsg { fd, msg, flags, _mark } = op;

        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_SENDMSG as _;
        assign_fd!(sqe.fd = fd);
        sqe.addr = msg as _;
        sqe.len = 1;
        sqe.__bindgen_anon_1.msg_flags = flags;
        Entry(sqe)
    }
}

impl From<RecvMsg> for Entry {
    fn from(op: RecvMsg) -> Entry {
        let RecvMsg { fd, msg, flags, _mark } = op;

        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_RECVMSG as _;
        assign_fd!(sqe.fd = fd);
        sqe.addr = msg as _;
        sqe.len = 1;
        sqe.__bindgen_anon_1.msg_flags = flags;
        Entry(sqe)
    }
}
