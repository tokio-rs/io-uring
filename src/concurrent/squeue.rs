use std::sync::atomic;

use crate::squeue::{self, Entry};
use crate::util::unsync_load;

use parking_lot::Mutex;

/// An io_uring instance's submission queue. This contains all the I/O operations the application
/// sends to the kernel.
pub struct SubmissionQueue<'a> {
    pub(crate) queue: &'a squeue::SubmissionQueue,
    pub(crate) push_lock: &'a Mutex<()>,
    pub(crate) ring_mask: u32,
    pub(crate) ring_entries: u32,
}

impl SubmissionQueue<'_> {
    /// When [`is_setup_sqpoll`](crate::Parameters::is_setup_sqpoll) is set, whether the kernel
    /// threads has gone to sleep and requires a system call to wake it up.
    #[inline]
    pub fn need_wakeup(&self) -> bool {
        self.queue.need_wakeup()
    }

    /// The number of invalid submission queue entries that have been encountered in the ring
    /// buffer.
    #[inline]
    pub fn dropped(&self) -> u32 {
        self.queue.dropped()
    }

    /// Get the total number of entries in the submission queue ring buffer.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.ring_entries as usize
    }

    /// Get the number of submission queue events in the ring buffer.
    #[inline]
    pub fn len(&self) -> usize {
        unsafe {
            let head = (*self.queue.head).load(atomic::Ordering::Acquire);
            let tail = (*self.queue.tail).load(atomic::Ordering::Acquire);

            tail.wrapping_sub(head) as usize
        }
    }

    /// Returns `true` if the submission queue ring buffer is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns `true` if the submission queue ring buffer has reached capacity, and no more events
    /// can be added before the kernel consumes some.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }

    /// Attempts to push an [`Entry`] into the queue.
    /// If pushing the entry was successful, this function returns `true`. If the queue was full,
    /// this function returns `false`.
    ///
    /// # Safety
    ///
    /// Developers must ensure that parameters of the [`Entry`] (such as buffer) are valid and will
    /// be valid for the entire duration of the operation, otherwise it may cause memory problems.
    pub unsafe fn push(&self, entry: Entry) -> bool {
        self.push_multiple(&[entry])
    }

    /// Attempts to push multiple [`Entry`] submissions into the queue.
    /// If pushing the entries was successful, this function returns `true`. If the queue was full,
    /// this function returns `false`.
    ///
    /// This operation will complete atomically, so no submissions can be submitted in between the
    /// two. This is useful for example when sending two [linked SQEs](squeue::Flags::IO_LINK).
    ///
    /// # Safety
    ///
    /// Developers must ensure that parameters of the [`Entry`] (such as buffer) are valid and will
    /// be valid for the entire duration of the operation, otherwise it may cause memory problems.
    pub unsafe fn push_multiple(&self, entries: &[Entry]) -> bool {
        let _lock = self.push_lock.lock();

        let head = (*self.queue.head).load(atomic::Ordering::Acquire);
        // We can load without synchronization because the only time the tail is modified is inside
        // this function, which we have the lock on.
        let mut tail = unsync_load(self.queue.tail);

        if self.capacity() - (tail.wrapping_sub(head) as usize) < entries.len() {
            return false;
        }

        for &Entry(entry) in entries {
            *self.queue.sqes.add((tail & self.ring_mask) as usize) = entry;
            tail = tail.wrapping_add(1);
        }

        (*self.queue.tail).store(tail, atomic::Ordering::Release);

        true
    }
}
