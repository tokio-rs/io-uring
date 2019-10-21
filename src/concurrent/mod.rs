pub mod squeue;
pub mod cqueue;

use std::io;
use std::sync::atomic;
use squeue::SubmissionQueue;
use cqueue::CompletionQueue;


pub struct IoUring {
    ring: crate::IoUring,
    mark: atomic::AtomicU32,
}

unsafe impl Send for IoUring {}
unsafe impl Sync for IoUring {}

impl IoUring {
    pub fn new(ring: crate::IoUring) -> IoUring {
        IoUring {
            ring,
            mark: atomic::AtomicU32::new(0)
        }
    }

    pub unsafe fn enter(&self, to_submit: u32, min_complete: u32, flag: u32, sig: Option<&libc::sigset_t>)
        -> io::Result<usize>
    {
        self.ring.enter(to_submit, min_complete, flag, sig)
    }

    pub fn submit(&self) -> io::Result<usize> {
        self.ring.submit()
    }

    pub fn submit_and_wait(&self, want: usize) -> io::Result<usize> {
        let len = self.ring.sq.len();

        self.ring.inner_enter(len, want)
    }

    pub fn submission(&self) -> SubmissionQueue<'_> {
        SubmissionQueue {
            queue: &self.ring.sq,
            mark: &self.mark,
            ring_mask: unsafe { *self.ring.sq.ring_mask },
            ring_entries: unsafe { *self.ring.sq.ring_entries },
        }
    }

    pub fn completion(&self) -> CompletionQueue<'_> {
        CompletionQueue {
            queue: &self.ring.cq,
            ring_mask: unsafe { *self.ring.cq.ring_mask },
            ring_entries: unsafe { *self.ring.cq.ring_entries }
        }
    }
}
