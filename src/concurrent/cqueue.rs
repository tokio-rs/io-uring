use std::sync::atomic;

use crate::cqueue::{self, Entry};

/// The completion queue of a concurrent io_uring instance. This stores all the I/O operations that
/// have completed.
pub struct CompletionQueue<'a> {
    pub(crate) queue: &'a cqueue::CompletionQueue,
    pub(crate) ring_mask: u32,
    pub(crate) ring_entries: u32,
}

impl CompletionQueue<'_> {
    /// If queue is full and [`is_feature_nodrop`](crate::Parameters::is_feature_nodrop) is not set,
    /// new events may be dropped. This records the number of dropped events.
    #[inline]
    pub fn overflow(&self) -> u32 {
        self.queue.overflow()
    }

    /// Get the total number of entries in the completion queue ring buffer.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.ring_entries as usize
    }

    /// Get the number of unread completion queue events in the ring buffer.
    #[inline]
    pub fn len(&self) -> usize {
        unsafe {
            let head = (*self.queue.head).load(atomic::Ordering::Acquire);
            let tail = (*self.queue.tail).load(atomic::Ordering::Acquire);

            tail.wrapping_sub(head) as usize
        }
    }

    /// Returns `true` if there are no completion queue events to be processed.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns `true` if the completion queue is at maximum capacity. If
    /// [`is_feature_nodrop`](crate::Parameters::is_feature_nodrop) is not set, this will cause any
    /// new completion queue events to be dropped by the kernel.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }

    /// Take the oldest completion queue event out of the queue. This can be called from multiple
    /// threads simultaneously.
    pub fn pop(&self) -> Option<Entry> {
        unsafe {
            loop {
                let head = (*self.queue.head).load(atomic::Ordering::Acquire);
                let tail = (*self.queue.tail).load(atomic::Ordering::Acquire);

                if head == tail {
                    return None;
                }

                let entry = *self.queue.cqes.add((head & self.ring_mask) as usize);

                match (*self.queue.head).compare_exchange_weak(
                    head,
                    head.wrapping_add(1),
                    atomic::Ordering::Release,
                    atomic::Ordering::Relaxed,
                ) {
                    Ok(_) => return Some(Entry(entry)),
                    Err(_) => continue,
                }
            }
        }
    }
}
