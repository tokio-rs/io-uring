use std::sync::atomic;

use crate::cqueue::{self, Entry};

pub struct CompletionQueue<'a> {
    pub(crate) queue: &'a cqueue::CompletionQueue,
    pub(crate) ring_mask: u32,
    pub(crate) ring_entries: u32,
}

impl CompletionQueue<'_> {
    #[inline]
    pub fn overflow(&self) -> u32 {
        self.queue.overflow()
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
