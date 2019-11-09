mod squeue;
mod cqueue;

use std::io;
use std::sync::atomic;
use crate::util::unsync_load;
pub use squeue::SubmissionQueue;
pub use cqueue::CompletionQueue;


pub struct IoUring {
    ring: crate::IoUring,
    mark: atomic::AtomicU32,
}

unsafe impl Send for IoUring {}
unsafe impl Sync for IoUring {}

impl IoUring {
    pub fn new(ring: crate::IoUring) -> IoUring {
        let mark = unsafe { unsync_load(ring.sq.tail) };

        IoUring {
            ring,
            mark: atomic::AtomicU32::new(mark)
        }
    }

    /// Initiate and/or complete asynchronous I/O
    ///
    /// # Safety
    ///
    /// This provides a raw interface so developer must ensure that parameters are correct.
    pub unsafe fn enter(&self, to_submit: u32, min_complete: u32, flag: u32, sig: Option<&libc::sigset_t>)
        -> io::Result<usize>
    {
        self.ring.enter(to_submit, min_complete, flag, sig)
    }

    /// Initiate `SubmissionQueue`.
    pub fn submit(&self) -> io::Result<usize> {
        self.ring.submit()
    }

    /// Initiate and/or complete asynchronous I/O
    pub fn submit_and_wait(&self, want: usize) -> io::Result<usize> {
        self.ring.submit_and_wait(want)
    }

    pub fn submission(&self) -> SubmissionQueue<'_> {
        SubmissionQueue {
            queue: &self.ring.sq,
            mark: &self.mark,
            ring_mask: unsafe { self.ring.sq.ring_mask.read_volatile() },
            ring_entries: unsafe { self.ring.sq.ring_entries.read_volatile() },
        }
    }

    pub fn completion(&self) -> CompletionQueue<'_> {
        CompletionQueue {
            queue: &self.ring.cq,
            ring_mask: unsafe { self.ring.cq.ring_mask.read_volatile() },
            ring_entries: unsafe { self.ring.cq.ring_entries.read_volatile() }
        }
    }

    pub fn into_inner(self) -> crate::IoUring {
        self.ring
    }
}
