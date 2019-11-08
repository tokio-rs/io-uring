pub mod cqueue;
pub mod squeue;

use cqueue::CompletionQueue;
use squeue::SubmissionQueue;
use std::io;

pub struct IoUring {
    ring: crate::IoUring,
}

unsafe impl Send for IoUring {}
unsafe impl Sync for IoUring {}

impl IoUring {
    pub fn new(ring: crate::IoUring) -> IoUring {
        IoUring { ring }
    }

    pub unsafe fn enter(
        &self,
        to_submit: u32,
        min_complete: u32,
        flag: u32,
        sig: Option<&libc::sigset_t>,
    ) -> io::Result<usize> {
        self.ring.enter(to_submit, min_complete, flag, sig)
    }

    pub fn submit(&self) -> io::Result<usize> {
        self.ring.submit()
    }

    pub fn submit_and_wait(&self, want: usize) -> io::Result<usize> {
        self.ring.submit_and_wait(want)
    }

    pub fn submission(&self) -> SubmissionQueue<'_> {
        SubmissionQueue {
            queue: &self.ring.sq,
            ring_mask: unsafe { self.ring.sq.ring_mask.read_volatile() },
            ring_entries: unsafe { self.ring.sq.ring_entries.read_volatile() },
        }
    }

    pub fn completion(&self) -> CompletionQueue<'_> {
        CompletionQueue {
            queue: &self.ring.cq,
            ring_mask: unsafe { self.ring.cq.ring_mask.read_volatile() },
            ring_entries: unsafe { self.ring.cq.ring_entries.read_volatile() },
        }
    }

    pub fn into_inner(self) -> crate::IoUring {
        self.ring
    }
}
