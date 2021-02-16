use crate::{cqueue, squeue, CompletionQueue, Inner, IoUring, SubmissionQueue, Submitter};
use std::sync::{atomic, Arc};

#[derive(Clone)]
pub struct SubmitterUring {
    inner: Arc<Inner>,

    sq_head: *const atomic::AtomicU32,
    sq_tail: *const atomic::AtomicU32,
    sq_flags: *const atomic::AtomicU32,
}

pub struct SubmissionUring {
    _inner: Arc<Inner>,
    sq: SubmissionQueue,
}

pub struct CompletionUring {
    _inner: Arc<Inner>,
    cq: CompletionQueue,
}

/// Safety: This only allows execution of syscall and atomic reads, so it is thread-safe.
unsafe impl Send for SubmitterUring {}
unsafe impl Sync for SubmitterUring {}

unsafe impl Send for SubmissionUring {}
unsafe impl Send for CompletionUring {}

pub(crate) fn split(ring: IoUring) -> (SubmitterUring, SubmissionUring, CompletionUring) {
    let inner = Arc::new(ring.inner);
    let inner2 = Arc::clone(&inner);
    let inner3 = Arc::clone(&inner);

    let stu = SubmitterUring {
        inner,

        sq_head: ring.sq.head,
        sq_tail: ring.sq.tail,
        sq_flags: ring.sq.flags,
    };
    let su = SubmissionUring {
        _inner: inner2,
        sq: ring.sq,
    };
    let cu = CompletionUring {
        _inner: inner3,
        cq: ring.cq,
    };

    (stu, su, cu)
}

impl SubmitterUring {
    /// Get the submitter of this io_uring instance, which can be used to submit submission queue
    /// events to the kernel for execution and to register files or buffers with it.
    #[inline]
    pub fn submitter(&self) -> Submitter<'_> {
        Submitter::new(
            &self.inner.fd,
            &self.inner.params,
            self.sq_head,
            self.sq_tail,
            self.sq_flags,
        )
    }
}

impl SubmissionUring {
    /// Get the submission queue of the io_uring instace. This is used to send I/O requests to the
    /// kernel.
    pub fn submission_mut(&mut self) -> squeue::Mut<'_> {
        squeue::Mut(&mut self.sq)
    }
}

impl CompletionUring {
    /// Get completion queue. This is used to receive I/O completion events from the kernel.
    pub fn completion_mut(&mut self) -> cqueue::Mut<'_> {
        cqueue::Mut(&mut self.cq)
    }
}
