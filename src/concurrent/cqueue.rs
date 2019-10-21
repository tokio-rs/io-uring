use std::sync::atomic;
use crate::cqueue::{ self, Entry };


pub struct CompletionQueue<'a> {
    pub(crate) queue: &'a cqueue::CompletionQueue,
    pub(crate) ring_mask: u32,
}

impl CompletionQueue<'_> {
    #[inline]
    pub fn overflow(&self) -> u32 {
        self.queue.overflow()
    }

    pub fn pop(&self) -> Option<Entry> {
        unsafe {
            loop {
                let head = (*self.queue.head).load(atomic::Ordering::Acquire);
                let tail = (*self.queue.tail).load(atomic::Ordering::Acquire);

                if head == tail {
                    return None;
                }

                let entry = self.queue.cqes.add((head & self.ring_mask) as usize);

                let new_head = head.wrapping_add(1);
                if (*self.queue.head).compare_and_swap(head, new_head, atomic::Ordering::Release)
                    == head
                {
                    return Some(Entry(*entry));
                }
            }
        }
    }
}
