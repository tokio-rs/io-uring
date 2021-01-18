//! Concurrent IoUring.
//!
//! This allows io_uring to be used from several threads at once.

mod cqueue;
mod squeue;

use std::io;
use std::sync::atomic;

pub use cqueue::CompletionQueue;
pub use squeue::SubmissionQueue;

use crate::util::unsync_load;

/// Concurrent IoUring instance
///
/// You can create it with [`IoUring::concurrent`](crate::IoUring::concurrent).
pub struct IoUring {
    ring: crate::IoUring,
    /// The index in the submission queue up to which entries are reserved. They are either filled
    /// in, or a thread is currently filling them in.
    sq_reserved_tail: atomic::AtomicU32,
}

unsafe impl Send for IoUring {}
unsafe impl Sync for IoUring {}

impl IoUring {
    pub(crate) fn new(mut ring: crate::IoUring) -> IoUring {
        let tail = unsafe { unsync_load(ring.submission().tail) };
        IoUring {
            ring,
            sq_reserved_tail: atomic::AtomicU32::new(tail),
        }
    }

    /// Initiate and/or complete asynchronous I/O. See
    /// [`Submitter::enter`](crate::Submitter::enter) for more details.
    ///
    /// # Safety
    ///
    /// This provides a raw interface so developer must ensure that parameters are correct.
    #[inline]
    pub unsafe fn enter(
        &self,
        to_submit: u32,
        min_complete: u32,
        flag: u32,
        sig: Option<&libc::sigset_t>,
    ) -> io::Result<usize> {
        self.ring.enter(to_submit, min_complete, flag, sig)
    }

    /// Submit all queued submission queue events to the kernel.
    #[inline]
    pub fn submit(&self) -> io::Result<usize> {
        self.ring.submit()
    }

    /// Submit all queued submission queue events to the kernel and wait for at least `want`
    /// completion events to complete.
    #[inline]
    pub fn submit_and_wait(&self, want: usize) -> io::Result<usize> {
        self.ring.submit_and_wait(want)
    }

    /// Get the submission queue of the io_uring instace. This is used to send I/O requests to the
    /// kernel.
    pub fn submission(&self) -> SubmissionQueue<'_> {
        unsafe {
            SubmissionQueue {
                queue: &self.ring.sq,
                reserved_tail: &self.sq_reserved_tail,
                ring_mask: self.ring.sq.ring_mask.read(),
                ring_entries: self.ring.sq.ring_entries.read(),
            }
        }
    }

    /// Get completion queue. This is used to receive I/O completion events from the kernel.
    pub fn completion(&self) -> CompletionQueue<'_> {
        unsafe {
            CompletionQueue {
                queue: &self.ring.cq,
                ring_mask: self.ring.cq.ring_mask.read(),
                ring_entries: self.ring.cq.ring_entries.read(),
            }
        }
    }

    /// Get the original [`IoUring`](crate::IoUring) instance.
    pub fn into_inner(self) -> crate::IoUring {
        self.ring
    }
}
