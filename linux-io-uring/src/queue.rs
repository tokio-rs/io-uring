use std::{ io, mem };
use std::os::unix::io::RawFd;
use linux_io_uring_sys as sys;
use crate::util::{ Mmap, Fd };


macro_rules! mmap_offset {
    ( $mmap:ident + $offset:expr => $ty:ty ) => {
        $mmap.as_mut_ptr().add($offset as _) as *const $ty
    };
    ( unsafe $( let $val:ident = $mmap:ident + $offset:expr => $ty:ty );+ $(;)? ) => {
        $(
            let $val = unsafe { mmap_offset!($mmap + $offset => $ty) };
        )*
    }
}

pub struct SubmissionQueue {
    _sq_mmap: Mmap,
    _sqe_mmap: Mmap,

    head: *const u32,
    tail: *const u32,
    ring_mask: *const u32,
    ring_entries: *const u32,
    flags: *const u32,
    dropped: *const u32,
    array: *const u32,

    sqes: *const sys::io_uring_sqe
}

pub struct CompletionQueue {
    _cq_mmap: Mmap,

    head: *const u32,
    tail: *const u32,
    ring_mask: *const u32,
    ring_entries: *const u32,
    overflow: *const u32,

    cqes: *const sys::io_uring_cqe
}

impl SubmissionQueue {
    pub fn new(fd: &Fd, p: &sys::io_uring_params) -> io::Result<SubmissionQueue> {
        let sq_mmap = Mmap::new(
            &fd,
            sys::IORING_OFF_SQ_RING as _,
            p.sq_off.array as usize + p.sq_entries as usize * mem::size_of::<u32>()
        )?;
        let sqe_mmap = Mmap::new(
            &fd,
            sys::IORING_OFF_SQES as _,
            p.sq_entries as usize * mem::size_of::<sys::io_uring_sqe>()
        )?;

        mmap_offset!{ unsafe
            let head            = sq_mmap + p.sq_off.head           => u32;
            let tail            = sq_mmap + p.sq_off.tail           => u32;
            let ring_mask       = sq_mmap + p.sq_off.ring_mask      => u32;
            let ring_entries    = sq_mmap + p.sq_off.ring_entries   => u32;
            let flags           = sq_mmap + p.sq_off.flags          => u32;
            let dropped         = sq_mmap + p.sq_off.dropped        => u32;
            let array           = sq_mmap + p.sq_off.array          => u32;

            let sqes            = sqe_mmap + 0                      => sys::io_uring_sqe;
        }

        Ok(SubmissionQueue {
            _sq_mmap: sq_mmap,
            _sqe_mmap: sqe_mmap,
            head, tail,
            ring_mask, ring_entries,
            flags,
            dropped,
            array,
            sqes
        })
    }
}

impl CompletionQueue {
    pub fn new(fd: &Fd, p: &sys::io_uring_params) -> io::Result<CompletionQueue> {
        let cq_mmap = Mmap::new(
            &fd,
            sys::IORING_OFF_CQ_RING as _,
            p.cq_off.cqes as usize + p.cq_entries as usize * mem::size_of::<sys::io_uring_cqe>()
        )?;

        mmap_offset!{ unsafe
            let head            = cq_mmap + p.cq_off.head           => u32;
            let tail            = cq_mmap + p.cq_off.tail           => u32;
            let ring_mask       = cq_mmap + p.cq_off.ring_mask      => u32;
            let ring_entries    = cq_mmap + p.cq_off.ring_entries   => u32;
            let overflow        = cq_mmap + p.cq_off.overflow       => u32;
            let cqes            = cq_mmap + p.cq_off.cqes           => sys::io_uring_cqe;
        }

        Ok(CompletionQueue {
            _cq_mmap: cq_mmap,
            head, tail,
            ring_mask, ring_entries,
            overflow,
            cqes
        })
    }
}
