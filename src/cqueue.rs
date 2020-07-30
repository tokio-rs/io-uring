//! Completion Queue

use std::sync::atomic;
use crate::util::{ Mmap, unsync_load };
use crate::sys;


pub struct CompletionQueue {
    pub(crate) head: *const atomic::AtomicU32,
    pub(crate) tail: *const atomic::AtomicU32,
    pub(crate) ring_mask: *const u32,
    pub(crate) ring_entries: *const u32,

    overflow: *const atomic::AtomicU32,

    pub(crate) cqes: *const sys::io_uring_cqe
}

/// Completion Entry
#[repr(transparent)]
#[derive(Clone)]
pub struct Entry(pub(crate) sys::io_uring_cqe);

pub struct AvailableQueue<'a> {
    head: u32,
    tail: u32,
    ring_mask: u32,
    ring_entries: u32,

    queue: &'a mut CompletionQueue
}

impl CompletionQueue {
    pub(crate) unsafe fn new(cq_mmap: &Mmap, p: &sys::io_uring_params) -> CompletionQueue {
        mmap_offset!{
            let head            = cq_mmap + p.cq_off.head           => *const atomic::AtomicU32;
            let tail            = cq_mmap + p.cq_off.tail           => *const atomic::AtomicU32;
            let ring_mask       = cq_mmap + p.cq_off.ring_mask      => *const u32;
            let ring_entries    = cq_mmap + p.cq_off.ring_entries   => *const u32;
            let overflow        = cq_mmap + p.cq_off.overflow       => *const atomic::AtomicU32;
            let cqes            = cq_mmap + p.cq_off.cqes           => *const sys::io_uring_cqe;
        }

        CompletionQueue {
            head, tail,
            ring_mask, ring_entries,
            overflow,
            cqes
        }
    }

    /// If queue is full, the new event maybe dropped.
    /// This value records number of dropped events.
    pub fn overflow(&self) -> u32 {
        unsafe {
            (*self.overflow).load(atomic::Ordering::Acquire)
        }
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        unsafe {
            self.ring_entries.read() as usize
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        unsafe {
            let head = unsync_load(self.head);
            let tail = (*self.tail).load(atomic::Ordering::Acquire);

            tail.wrapping_sub(head) as usize
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }

    /// Get currently available completion queue
    pub fn available(&mut self) -> AvailableQueue<'_> {
        unsafe {
            AvailableQueue {
                head: unsync_load(self.head),
                tail: (*self.tail).load(atomic::Ordering::Acquire),
                ring_mask: self.ring_mask.read(),
                ring_entries: self.ring_entries.read(),
                queue: self
            }
        }
    }
}

impl AvailableQueue<'_> {
    /// Sync queue
    pub fn sync(&mut self) {
        unsafe {
            (*self.queue.head).store(self.head, atomic::Ordering::Release);
            self.tail = (*self.queue.tail).load(atomic::Ordering::Acquire);
        }
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.ring_entries as usize
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }
}

impl ExactSizeIterator for AvailableQueue<'_> {
    #[inline]
    fn len(&self) -> usize {
        self.tail.wrapping_sub(self.head) as usize
    }
}

impl Iterator for AvailableQueue<'_> {
    type Item = Entry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.head != self.tail {
            unsafe {
                let entry = self.queue.cqes.add((self.head & self.ring_mask) as usize);
                self.head = self.head.wrapping_add(1);
                Some(Entry(*entry))
            }
        } else {
            None
        }
    }
}

impl Drop for AvailableQueue<'_> {
    fn drop(&mut self) {
        unsafe {
            (*self.queue.head).store(self.head, atomic::Ordering::Release);
        }
    }
}

impl Entry {
    /// Result value
    pub fn result(&self) -> i32 {
        self.0.res
    }

    /// User Data
    ///
    /// See [Entry::user_data](crate::squeue::Entry::user_data).
    pub fn user_data(&self) -> u64 {
        self.0.user_data
    }

    /// Flags
    pub fn flags(&self) -> u32 {
        self.0.flags
    }
}
