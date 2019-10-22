use std::os::unix::io::RawFd;
use linux_io_uring_sys as sys;
use crate::squeue::Entry;


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

macro_rules! opcode {
    (
        $( #[$outer:meta] )*
        pub struct $name:ident {
            $( $field:ident : $tname:ty ),* $(,)?
            ;;
            $( $opt_field:ident : $opt_tname:ty = $default:expr ),* $(,)?
        }
    ) => {
        $( #[$outer] )*
        pub struct $name {
            $( $field : $tname, )*
            $( $opt_field : $opt_tname, )*
        }

        impl $name {
            pub fn new( $( $field : $tname ),* ) -> Self {
                $name {
                    $( $field , )*
                    $( $opt_field: $default, )*
                }
            }

            $(
                pub fn $opt_field<T: Into<$opt_tname>>(mut self, $opt_field: T) -> Self {
                    self.$opt_field = $opt_field.into();
                    self
                }
            )*
        }
    }
}


#[derive(Debug, Clone)]
pub enum Target {
    Fd(RawFd),
    Fixed(u32)
}

// TODO(@quininer)
//
// * derive Debug
// * document

opcode!(
    pub struct Nop { ;; }
);

opcode!(
    pub struct Readv {
        fd: Target,
        iovec: *mut libc::iovec,
        len: u32,
        ;;
        offset: i64 = 0,
        rw_flags: i32 = 0
    }
);

opcode!(
    pub struct Writev {
        fd: Target,
        iovec: *const libc::iovec,
        len: u32,
        ;;
        offset: i64 = 0,
        rw_flags: i32 = 0
    }
);

opcode!(
    pub struct Fsync {
        fd: Target,
        ;;
        flags: u32 = 0
    }
);

opcode!(
    pub struct ReadFixed {
        fd: Target,
        buf: *mut u8,
        len: u32,
        buf_index: u16,
        ;;
        offset: i64 = 0,
        rw_flags: i32 = 0
    }
);

opcode!(
    pub struct WriteFixed {
        fd: Target,
        buf: *const u8,
        len: u32,
        buf_index: u16,
        ;;
        offset: i64 = 0,
        rw_flags: i32 = 0
    }
);

opcode!(
    pub struct PollAdd {
        fd: Target,
        mask: u16,
        ;;
    }
);

opcode!(
    pub struct PollRemove {
        user_data: *const libc::c_void
        ;;
    }
);

opcode!(
    pub struct SyncFileRange {
        fd: Target,
        len: u32,
        ;;
        offset: i64 = 0,
        flags: u32 = 0
    }
);

opcode!(
    pub struct SendMsg {
        fd: Target,
        msg: *const libc::msghdr,
        ;;
        flags: u32 = 0
    }
);

opcode!(
    pub struct RecvMsg {
        fd: Target,
        msg: *mut libc::msghdr,
        ;;
        flags: u32 = 0
    }
);

impl From<Nop> for Entry {
    fn from(op: Nop) -> Entry {
        let Nop {} = op;

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
            rw_flags
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
            rw_flags
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
        let Fsync { fd, flags } = op;

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
            buf_index,
            rw_flags
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
            buf_index,
            rw_flags
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
        let PollAdd { fd, mask } = op;

        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_POLL_ADD as _;
        assign_fd!(sqe.fd = fd);
        sqe.__bindgen_anon_1.poll_events = mask as _;
        Entry(sqe)
    }
}

impl From<PollRemove> for Entry {
    fn from(op: PollRemove) -> Entry {
        let PollRemove { user_data } = op;

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
            flags
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
        let SendMsg { fd, msg, flags } = op;

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
        let RecvMsg { fd, msg, flags } = op;

        let mut sqe = sys::io_uring_sqe::default();
        sqe.opcode = sys::IORING_OP_RECVMSG as _;
        assign_fd!(sqe.fd = fd);
        sqe.addr = msg as _;
        sqe.len = 1;
        sqe.__bindgen_anon_1.msg_flags = flags;
        Entry(sqe)
    }
}
