//! The `io_uring` library for Rust.
//!
//! The crate only provides a summary of the parameters.
//! For more detailed documentation, see manpage.

mod util;
mod register;
pub mod squeue;
pub mod cqueue;
pub mod opcode;
pub mod submit;

#[cfg(feature = "concurrent")]
pub mod concurrent;

use std::{ io, cmp, mem };
use std::convert::TryInto;
use std::os::unix::io::{ AsRawFd, RawFd };
use std::mem::ManuallyDrop;
use linux_io_uring_sys as sys;
use util::{ Fd, Mmap };
pub use submit::Submitter;
pub use squeue::SubmissionQueue;
pub use cqueue::CompletionQueue;
pub use register::{ register as reg, unregister as unreg };


/// IoUring instance
pub struct IoUring {
    fd: Fd,
    flags: u32,
    memory: ManuallyDrop<MemoryMap>,
    sq: SubmissionQueue,
    cq: CompletionQueue
}

#[allow(dead_code)]
struct MemoryMap {
    sq_mmap: Mmap,
    sqe_mmap: Mmap,
    cq_mmap: Option<Mmap>
}

/// IoUring Builder
#[derive(Clone)]
pub struct Builder {
    entries: u32,
    params: sys::io_uring_params
}

unsafe impl Send for IoUring {}
unsafe impl Sync for IoUring {}

impl IoUring {
    /// Create a IoUring instance
    ///
    /// The `entries` sets the size of queue,
    /// and it value should be the power of two.
    #[inline]
    pub fn new(entries: u32) -> io::Result<IoUring> {
        IoUring::with_params(entries, sys::io_uring_params::default())
    }

    fn with_params(entries: u32, mut p: sys::io_uring_params) -> io::Result<IoUring> {
        // NOTE: The `SubmissionQueue` and `CompletionQueue` are references,
        // and their lifetime can never exceed `MemoryMap`.
        //
        // I really hope that Rust can safely use self-reference types.
        #[inline]
        unsafe fn setup_queue(fd: &Fd, p: &sys::io_uring_params)
            -> io::Result<(MemoryMap, SubmissionQueue, CompletionQueue)>
        {
            let sq_len = p.sq_off.array as usize
                + p.sq_entries as usize * mem::size_of::<u32>();
            let cq_len = p.cq_off.cqes as usize
                + p.cq_entries as usize * mem::size_of::<sys::io_uring_cqe>();
            let sqe_len = p.sq_entries as usize * mem::size_of::<sys::io_uring_sqe>();
            let sqe_mmap = Mmap::new(fd, sys::IORING_OFF_SQES as _, sqe_len)?;

            if p.features & sys::IORING_FEAT_SINGLE_MMAP != 0 {
                let scq_mmap = Mmap::new(fd, sys::IORING_OFF_SQ_RING as _, cmp::max(sq_len, cq_len))?;

                let sq = SubmissionQueue::new(&scq_mmap, &sqe_mmap, p);
                let cq = CompletionQueue::new(&scq_mmap, p);
                let mm = MemoryMap {
                    sq_mmap: scq_mmap,
                    cq_mmap: None,
                    sqe_mmap
                };

                Ok((mm, sq, cq))
            } else {
                let sq_mmap = Mmap::new(fd, sys::IORING_OFF_SQ_RING as _, sq_len)?;
                let cq_mmap = Mmap::new(fd, sys::IORING_OFF_CQ_RING as _, cq_len)?;

                let sq = SubmissionQueue::new(&sq_mmap, &sqe_mmap, p);
                let cq = CompletionQueue::new(&cq_mmap, p);
                let mm = MemoryMap {
                    cq_mmap: Some(cq_mmap),
                    sq_mmap, sqe_mmap
                };

                Ok((mm, sq, cq))
            }
        }

        let fd: Fd = unsafe {
            sys::io_uring_setup(entries, &mut p)
                .try_into()
                .map_err(|_| io::Error::last_os_error())?
        };

        let flags = p.flags;
        let (mm, sq, cq) = unsafe { setup_queue(&fd, &p)? };

        Ok(IoUring {
            fd, flags, sq, cq,
            memory: ManuallyDrop::new(mm)
        })
    }

    const fn as_submit(&self) -> Submitter<'_> {
        Submitter::new(&self.fd, self.flags, &self.sq)
    }

    /// Register files or user buffers for asynchronous I/O.
    #[inline]
    pub fn register(&self, target: reg::Target<'_>) -> io::Result<()> {
        self.as_submit().register(target)
    }

    /// Unregister files or user buffers for asynchronous I/O.
    #[inline]
    pub fn unregister(&self, target: unreg::Target) -> io::Result<()> {
        self.as_submit().unregister(target)
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
        self.as_submit().enter(to_submit, min_complete, flag, sig)
    }

    /// Initiate asynchronous I/O.
    #[inline]
    pub fn submit(&self) -> io::Result<usize> {
        self.as_submit().submit()
    }

    /// Initiate and/or complete asynchronous I/O
    #[inline]
    pub fn submit_and_wait(&self, want: usize) -> io::Result<usize> {
        self.as_submit().submit_and_wait(want)
    }

    /// Get submitter and submission queue and completion queue
    pub fn split(&mut self)
        -> (Submitter<'_>, &mut SubmissionQueue, &mut CompletionQueue)
    {
        let submit = Submitter::new(&self.fd, self.flags, &self.sq);
        (submit, &mut self.sq, &mut self.cq)
    }

    /// Get submission queue
    pub fn submission(&mut self) -> &mut SubmissionQueue {
        &mut self.sq
    }

    /// Get completion queue
    pub fn completion(&mut self) -> &mut CompletionQueue {
        &mut self.cq
    }

    /// Make a concurrent IoUring.
    #[cfg(feature = "concurrent")]
    pub fn concurrent(self) -> concurrent::IoUring {
        concurrent::IoUring::new(self)
    }
}

impl Drop for IoUring {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.memory);
        }
    }
}

impl Builder {
    /// Create a builder
    pub fn new(entries: u32) -> Self {
        Builder {
            entries,
            params: sys::io_uring_params::default()
        }
    }

    pub fn feature_single_mmap(mut self) -> Self {
        self.params.features |= sys::IORING_FEAT_SINGLE_MMAP;
        self
    }

    #[cfg(feature = "unstable")]
    pub fn feature_nodrop(mut self) -> Self {
        self.params.features |= sys::IORING_FEAT_NODROP;
        self
    }

    #[cfg(feature = "unstable")]
    pub fn feature_submit_stable(mut self) -> Self {
        self.params.features |= sys::IORING_FEAT_SUBMIT_STABLE;
        self
    }

    /// Perform busy-waiting for an I/O completion,
    /// as opposed to getting notifications via an asynchronous IRQ (Interrupt Request).
    pub fn setup_iopoll(mut self) -> Self {
        self.params.flags |= sys::IORING_SETUP_IOPOLL;
        self
    }

    /// When this flag is specified, a kernel thread is created to perform submission queue polling.
    /// An io_uring instance configured in this way enables an application to issue I/O
    /// without ever context switching into the kernel.
    pub fn setup_sqpoll(mut self, idle: impl Into<Option<u32>>) -> Self {
        self.params.flags |= sys::IORING_SETUP_SQPOLL;

        if let Some(n) = idle.into() {
            self.params.sq_thread_idle = n;
        }

        self
    }

    /// If this flag is specified,
    /// then the poll thread will be bound to the cpu set in the value.
    /// This flag is only meaningful when [Builder::setup_sqpoll] is enabled.
    pub fn setup_sqpoll_cpu(mut self, n: u32) -> Self {
        self.params.sq_thread_cpu = n;
        self
    }

    pub fn build(self) -> io::Result<IoUring> {
        IoUring::with_params(self.entries, self.params)
    }
}

impl AsRawFd for IoUring {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}
