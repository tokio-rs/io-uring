use std::sync::atomic;
use parking_lot::Mutex;
use crate::util::unsync_load;
use crate::squeue::{ self, Entry };


pub struct SubmissionQueue<'a> {
    pub(crate) queue: &'a squeue::SubmissionQueue,
    pub(crate) push_lock: &'a Mutex<()>,
    pub(crate) ring_mask: u32,
    pub(crate) ring_entries: u32
}

impl SubmissionQueue<'_> {
    #[inline]
    pub fn need_wakeup(&self) -> bool {
        self.queue.need_wakeup()
    }

    #[inline]
    pub fn dropped(&self) -> u32 {
        self.queue.dropped()
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.ring_entries as usize
    }

    #[inline]
    pub fn len(&self) -> usize {
        unsafe {
            let head = (*self.queue.head).load(atomic::Ordering::Acquire);
            let tail = (*self.queue.tail).load(atomic::Ordering::Acquire);

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

    /// Attempts to push an [Entry] into the queue.
    /// If the queue is full, the element is returned back as an error.
    ///
    /// # Safety
    ///
    /// Developers must ensure that parameters of the [Entry] (such as buffer) are valid,
    /// otherwise it may cause memory problems.
    pub unsafe fn push(&self, Entry(entry): Entry) -> Result<(), Entry> {
        let _lock = self.push_lock.lock();

        let head = (*self.queue.head).load(atomic::Ordering::Acquire);
        let tail = unsync_load(self.queue.tail);

        if tail.wrapping_sub(head) == self.ring_entries {
            return Err(Entry(entry));
        }

        *self.queue.sqes.add((tail & self.ring_mask) as usize)
            = entry;

        (*self.queue.tail).store(tail.wrapping_add(1), atomic::Ordering::Release);

        Ok(())
    }
}
