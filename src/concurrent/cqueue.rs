use std::sync::atomic;
use crate::cqueue::{ self, Entry };


pub struct CompletionQueue<'a> {
    pub(crate) queue: &'a cqueue::CompletionQueue,
    pub(crate) ring_mask: u32,
    pub(crate) ring_entries: u32
}

impl CompletionQueue<'_> {
    #[inline]
    pub fn overflow(&self) -> u32 {
        self.queue.overflow()
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

    pub fn is_full(&self) -> bool {
        self.len() == self.ring_entries as usize
    }

    pub fn pop(&self) -> Option<Entry> {
        unsafe {
            loop {
                let head = (*self.queue.head).load(atomic::Ordering::Acquire);
                let tail = (*self.queue.tail).load(atomic::Ordering::Acquire);

                if head == tail {
                    return None;
                }

                let entry = *self.queue.cqes.add((head & self.ring_mask) as usize);

                let new_head = head.wrapping_add(1);
                if (*self.queue.head).compare_and_swap(head, new_head, atomic::Ordering::Release)
                    == head
                {
                    return Some(Entry(entry));
                }
            }
        }
    }
}
