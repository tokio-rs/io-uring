use crate::{CompletionQueue, Inner, IoUring, SubmissionQueue, Submitter};
use std::sync::{atomic, Arc};

pub struct SubmissionUring {
    _inner: Arc<Inner>,
    sq: SubmissionQueue,
}

pub struct CompletionUring {
    inner: Arc<Inner>,
    cq: CompletionQueue,

    sq_head: *const atomic::AtomicU32,
    sq_tail: *const atomic::AtomicU32,
    sq_flags: *const atomic::AtomicU32,
}

pub(crate) fn split(ring: IoUring) -> (SubmissionUring, CompletionUring) {
    let inner = Arc::new(ring.inner);
    let inner2 = Arc::clone(&inner);

    let cu = CompletionUring {
        inner,
        cq: ring.cq,

        sq_head: ring.sq.head,
        sq_tail: ring.sq.tail,
        sq_flags: ring.sq.flags,
    };
    let su = SubmissionUring {
        _inner: inner2,
        sq: ring.sq,
    };

    (su, cu)
}

impl SubmissionUring {
    /// Get the submission queue of the io_uring instace. This is used to send I/O requests to the
    /// kernel.
    pub fn submission(&mut self) -> &mut SubmissionQueue {
        &mut self.sq
    }
}

impl CompletionUring {
    /// Get completion queue. This is used to receive I/O completion events from the kernel.
    pub fn completion(&mut self) -> &mut CompletionQueue {
        &mut self.cq
    }

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

    /// Get the submitter, completion queue of the io_uring instance. This can
    /// be used to operate on the different parts of the io_uring instance independently.
    pub fn split(&mut self) -> (Submitter<'_>, &mut CompletionQueue) {
        let submit = Submitter::new(
            &self.inner.fd,
            &self.inner.params,
            self.sq_head,
            self.sq_tail,
            self.sq_flags,
        );
        (submit, &mut self.cq)
    }
}
