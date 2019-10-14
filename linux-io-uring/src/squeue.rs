use std::{ io, mem, ptr };
use std::os::unix::io::RawFd;
use linux_io_uring_sys as sys;
use crate::util::{ Mmap, Fd };
use crate::mmap_offset;


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

pub struct Entry(sys::io_uring_sqe);

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

    fn last(&self) -> Option<*mut sys::io_uring_sqe> {
        unimplemented!()
    }

    pub fn is_empty(&self) -> bool {
        unimplemented!()
    }

    pub fn is_full(&self) -> bool {
        unimplemented!()
    }

    pub fn push(&self, Entry(entry): Entry) -> Result<(), Entry> {
        if let Some(p) = self.last() {
            unsafe {
                ptr::write(p, entry);
                Ok(())
            }
        } else {
            Err(Entry(entry))
        }
    }
}
