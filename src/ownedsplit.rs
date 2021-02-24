use crate::{CompletionQueue, IoUring, SubmissionQueue, Submitter};
use std::sync::Arc;

#[derive(Clone)]
pub struct SubmitterUring {
    inner: Arc<IoUring>,
}

pub struct SubmissionUring {
    inner: Arc<IoUring>,
}

pub struct CompletionUring {
    inner: Arc<IoUring>,
}

pub(crate) fn split(ring: IoUring) -> (SubmitterUring, SubmissionUring, CompletionUring) {
    let inner = Arc::new(ring);
    (
        SubmitterUring {
            inner: Arc::clone(&inner),
        },
        SubmissionUring {
            inner: Arc::clone(&inner),
        },
        CompletionUring { inner },
    )
}

impl SubmitterUring {
    /// Get the submitter of this io_uring instance, which can be used to submit submission queue
    /// events to the kernel for execution and to register files or buffers with it.
    #[inline]
    pub fn submitter(&self) -> Submitter<'_> {
        self.inner.submitter()
    }
}

impl SubmissionUring {
    /// Get the submission queue of the io_uring instace. This is used to send I/O requests to the
    /// kernel.
    pub fn submission(&mut self) -> SubmissionQueue<'_> {
        unsafe { self.inner.submission_shared() }
    }
}

impl CompletionUring {
    /// Get completion queue. This is used to receive I/O completion events from the kernel.
    pub fn completion(&mut self) -> CompletionQueue<'_> {
        unsafe { self.inner.completion_shared() }
    }
}
