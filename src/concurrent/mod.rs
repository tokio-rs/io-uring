//! Concurrent IoUring.

mod squeue;
mod cqueue;

use std::io;
use parking_lot::Mutex;
pub use squeue::SubmissionQueue;
pub use cqueue::CompletionQueue;


/// Concurrent IoUring instance
pub struct IoUring {
    ring: crate::IoUring,
    push_lock: Mutex<()>
}

unsafe impl Send for IoUring {}
unsafe impl Sync for IoUring {}

impl IoUring {
    pub(crate) fn new(ring: crate::IoUring) -> IoUring {
        IoUring {
            ring,
            push_lock: Mutex::new(())
        }
    }

    /// Initiate and/or complete asynchronous I/O
    ///
    /// # Safety
    ///
    /// This provides a raw interface so developer must ensure that parameters are correct.
    #[inline]
    pub unsafe fn enter(&self, to_submit: u32, min_complete: u32, flag: u32, sig: Option<&libc::sigset_t>)
        -> io::Result<usize>
    {
        self.ring.enter(to_submit, min_complete, flag, sig)
    }

    /// Initiate asynchronous I/O.
    #[inline]
    pub fn submit(&self) -> io::Result<usize> {
        self.ring.submit()
    }

    /// Initiate and/or complete asynchronous I/O
    #[inline]
    pub fn submit_and_wait(&self, want: usize) -> io::Result<usize> {
        self.ring.submit_and_wait(want)
    }

    /// Get submission queue
    pub fn submission(&self) -> SubmissionQueue<'_> {
        SubmissionQueue {
            queue: &self.ring.sq,
            push_lock: &self.push_lock,
            ring_mask: unsafe { self.ring.sq.ring_mask.read_volatile() },
            ring_entries: unsafe { self.ring.sq.ring_entries.read_volatile() },
        }
    }

    /// Get completion queue
    pub fn completion(&self) -> CompletionQueue<'_> {
        CompletionQueue {
            queue: &self.ring.cq,
            ring_mask: unsafe { self.ring.cq.ring_mask.read_volatile() },
            ring_entries: unsafe { self.ring.cq.ring_entries.read_volatile() }
        }
    }

    /// Get original IoUring instance
    pub fn into_inner(self) -> crate::IoUring {
        self.ring
    }
}
