use std::sync::atomic;
use crate::squeue::{ self, Entry };


pub struct SubmissionQueue<'a> {
    pub(crate) queue: &'a squeue::SubmissionQueue,
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

    pub fn len(&self) -> usize {
        let head = unsafe { (*self.queue.head).load(atomic::Ordering::Acquire) };
        let tail = unsafe { (*self.queue.tail).load(atomic::Ordering::Acquire) };

        tail.wrapping_sub(head) as usize
    }

    pub fn is_empty(&self) -> bool {
        let head = unsafe { (*self.queue.head).load(atomic::Ordering::Acquire) };
        let tail = unsafe { (*self.queue.tail).load(atomic::Ordering::Acquire) };

        head == tail
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }

    pub unsafe fn push(&self, Entry(entry): Entry) -> Result<(), Entry> {
        let tail = loop {
            let head = (*self.queue.head).load(atomic::Ordering::Acquire);
            let tail = (*self.queue.tail).load(atomic::Ordering::Acquire);

            if tail.wrapping_sub(head) == self.ring_entries {
                return Err(Entry(entry));
            }

            if (*self.queue.tail).compare_and_swap(tail, tail.wrapping_add(1), atomic::Ordering::Release)
                == tail
            {
                break tail;
            }
        };

        *self.queue.sqes.add((tail & self.ring_mask) as usize)
            = entry;

        Ok(())
    }
}
