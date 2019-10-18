use std::{ io, mem };
use std::os::unix::io::RawFd;
use linux_io_uring_sys as sys;
use crate::util::{ Mmap, Fd };
use crate::mmap_offset;


pub struct CompletionQueue {
    _cq_mmap: Mmap,

    head: *const u32,
    tail: *const u32,
    ring_mask: *const u32,
    ring_entries: *const u32,
    overflow: *const u32,

    cqes: *const sys::io_uring_cqe
}

impl CompletionQueue {
    pub(crate) fn new(fd: &Fd, p: &sys::io_uring_params) -> io::Result<CompletionQueue> {
        let cq_mmap = Mmap::new(
            &fd,
            sys::IORING_OFF_CQ_RING as _,
            p.cq_off.cqes as usize + p.cq_entries as usize * mem::size_of::<sys::io_uring_cqe>()
        )?;

        mmap_offset!{ unsafe
            let head            = cq_mmap + p.cq_off.head           => *const u32;
            let tail            = cq_mmap + p.cq_off.tail           => *const u32;
            let ring_mask       = cq_mmap + p.cq_off.ring_mask      => *const u32;
            let ring_entries    = cq_mmap + p.cq_off.ring_entries   => *const u32;
            let overflow        = cq_mmap + p.cq_off.overflow       => *const u32;
            let cqes            = cq_mmap + p.cq_off.cqes           => *const sys::io_uring_cqe;
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
