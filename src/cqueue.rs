use std::{ io, mem };
use std::sync::atomic;
use linux_io_uring_sys as sys;
use crate::util::{ Mmap, Fd, AtomicU32Ref };
use crate::mmap_offset;


pub struct CompletionQueue {
    _cq_mmap: Mmap,

    head: AtomicU32Ref,
    tail: *const atomic::AtomicU32,
    ring_mask: *const u32,

    #[allow(dead_code)]
    ring_entries: *const u32,

    overflow: *const atomic::AtomicU32,

    cqes: *const sys::io_uring_cqe
}

#[derive(Clone)]
pub struct Entry(sys::io_uring_cqe);

pub struct AvailableQueue<'a> {
    head: u32,
    tail: u32,
    ring_mask: u32,

    queue: &'a mut CompletionQueue
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
            let tail            = cq_mmap + p.cq_off.tail           => *const atomic::AtomicU32;
            let ring_mask       = cq_mmap + p.cq_off.ring_mask      => *const u32;
            let ring_entries    = cq_mmap + p.cq_off.ring_entries   => *const u32;
            let overflow        = cq_mmap + p.cq_off.overflow       => *const atomic::AtomicU32;
            let cqes            = cq_mmap + p.cq_off.cqes           => *const sys::io_uring_cqe;
        }

        Ok(CompletionQueue {
            _cq_mmap: cq_mmap,
            head: unsafe { AtomicU32Ref::new(head) },
            tail,
            ring_mask, ring_entries,
            overflow,
            cqes
        })
    }

    pub fn overflow(&self) -> u32 {
        unsafe {
            (*self.overflow).load(atomic::Ordering::Acquire)
        }
    }

    pub fn available<'a>(&'a mut self) -> AvailableQueue<'a> {
        unsafe {
            AvailableQueue {
                head: self.head.unsync_load(),
                tail: (*self.tail).load(atomic::Ordering::Acquire),
                ring_mask: *self.ring_mask,
                queue: self
            }
        }
    }
}

impl ExactSizeIterator for AvailableQueue<'_> {
    fn len(&self) -> usize {
        (self.tail - self.head) as usize
    }

    fn is_empty(&self) -> bool {
        self.head == self.tail
    }
}

impl Iterator for AvailableQueue<'_> {
    type Item = Entry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.head != self.tail {
            unsafe {
                let entry = self.queue.cqes.add((self.head & self.ring_mask) as usize);
                Some(Entry((*entry).clone()))
            }
        } else {
            None
        }
    }
}

impl Drop for AvailableQueue<'_> {
    fn drop(&mut self) {
        self.queue.head.store(self.head, atomic::Ordering::Release);
    }
}

impl Entry {
    pub fn result(&self) -> i32 {
        self.0.res
    }

    pub fn user_data(&self) -> u64 {
        self.0.user_data
    }
}
