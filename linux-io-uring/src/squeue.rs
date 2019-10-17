use std::{ io, mem, ptr };
use std::cell::Cell;
use std::sync::atomic;
use std::os::unix::io::RawFd;
use std::marker::PhantomData;
use linux_io_uring_sys as sys;
use crate::util::{ Mmap, Fd, AtomicU32Ref };
use crate::mmap_offset;


pub struct SubmissionQueue {
    _sq_mmap: Mmap,
    _sqe_mmap: Mmap,

    head: AtomicU32Ref,
    tail: AtomicU32Ref,
    ring_mask: *const u32,
    ring_entries: *const u32,
    flags: *const u32,
    dropped: *const u32,
    array: *const u32,

    sqes: *mut sys::io_uring_sqe
}

pub struct AvailableQueue<'a> {
    head: Cell<u32>,
    tail: u32,
    ring_mask: u32,
    ring_entries: u32,
    queue: &'a mut SubmissionQueue
}

pub struct Entry(pub sys::io_uring_sqe);

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
            let head            = sq_mmap + p.sq_off.head           => *const u32;
            let tail            = sq_mmap + p.sq_off.tail           => *const u32;
            let ring_mask       = sq_mmap + p.sq_off.ring_mask      => *const u32;
            let ring_entries    = sq_mmap + p.sq_off.ring_entries   => *const u32;
            let flags           = sq_mmap + p.sq_off.flags          => *const u32;
            let dropped         = sq_mmap + p.sq_off.dropped        => *const u32;
            let array           = sq_mmap + p.sq_off.array          => *const u32;

            let sqes            = sqe_mmap + 0                      => *mut sys::io_uring_sqe;
        }

        Ok(SubmissionQueue {
            _sq_mmap: sq_mmap,
            _sqe_mmap: sqe_mmap,
            head: unsafe { AtomicU32Ref::new(head) },
            tail: unsafe { AtomicU32Ref::new(tail) },
            ring_mask, ring_entries,
            flags,
            dropped,
            array,
            sqes
        })
    }

    pub fn is_full(&self) -> bool {
        unsafe {
            let head = self.head.load(atomic::Ordering::Acquire);
            let tail = self.tail.unsync_load();
            let ring_entries = *self.ring_entries;

            tail.wrapping_sub(head) >= ring_entries
        }
    }

    pub fn available<'a>(&'a mut self) -> AvailableQueue<'a> {
        unsafe {
            AvailableQueue {
                head: Cell::new(self.head.load(atomic::Ordering::Acquire)),
                tail: self.tail.unsync_load(),
                ring_mask: *self.ring_mask,
                ring_entries: *self.ring_entries,
                queue: self
            }
        }
    }
}

impl<'a> AvailableQueue<'a> {
    pub fn is_full(&self) -> bool {
        self.tail.wrapping_sub(self.head.get()) >= self.ring_entries
            && {
                let head = self.queue.head.load(atomic::Ordering::Acquire);
                self.head.set(head);
                self.tail.wrapping_sub(head) >= self.ring_entries
            }
    }

    pub unsafe fn push(&mut self, Entry(entry): Entry) -> Result<(), Entry> {
        if self.is_full() {
            Err(Entry(entry))
        } else {
            unsafe {
                *self.queue.sqes.add(self.tail as usize & self.ring_mask as usize)
                    = entry;
            }

            self.tail += 1;
            Ok(())
        }
    }
}

impl<'a> Drop for AvailableQueue<'a> {
    fn drop(&mut self) {
        self.queue.tail.store(self.tail, atomic::Ordering::Release);
    }
}
