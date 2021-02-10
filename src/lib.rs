//! The `io_uring` library for Rust.
//!
//! The crate only provides a summary of the parameters.
//! For more detailed documentation, see manpage.

#[macro_use]
mod util;
pub mod cqueue;
pub mod opcode;
mod register;
pub mod squeue;
mod submit;
mod sys;
pub mod types;

#[cfg(feature = "unstable")]
pub mod ownedsplit;

#[cfg(feature = "concurrent")]
pub mod concurrent;

use std::convert::TryInto;
use std::mem::ManuallyDrop;
use std::os::unix::io::{AsRawFd, RawFd};
use std::{cmp, io, mem};

pub use cqueue::CompletionQueue;
pub use register::Probe;
pub use squeue::SubmissionQueue;
pub use submit::Submitter;
use util::{Fd, Mmap};

/// IoUring instance
pub struct IoUring {
    inner: Inner,
    sq: SubmissionQueue,
    cq: CompletionQueue,
}

struct Inner {
    fd: Fd,
    params: Parameters,
    memory: ManuallyDrop<MemoryMap>,
}

#[allow(dead_code)]
struct MemoryMap {
    sq_mmap: Mmap,
    sqe_mmap: Mmap,
    cq_mmap: Option<Mmap>,
}

/// IoUring build params
#[derive(Clone, Default)]
pub struct Builder {
    dontfork: bool,
    params: sys::io_uring_params,
}

/// The parameters that were used to construct an [`IoUring`].
#[derive(Clone)]
pub struct Parameters(sys::io_uring_params);

unsafe impl Send for IoUring {}
unsafe impl Sync for IoUring {}

impl IoUring {
    /// Create a new `IoUring` instance with default configuration parameters. See [`Builder`] to
    /// customize it further.
    ///
    /// The `entries` sets the size of queue,
    /// and its value should be the power of two.
    #[inline]
    pub fn new(entries: u32) -> io::Result<IoUring> {
        IoUring::with_params(entries, Default::default())
    }

    fn with_params(entries: u32, mut p: sys::io_uring_params) -> io::Result<IoUring> {
        // NOTE: The `SubmissionQueue` and `CompletionQueue` are references,
        // and their lifetime can never exceed `MemoryMap`.
        //
        // The memory mapped regions of `MemoryMap` never move,
        // so `SubmissionQueue` and `CompletionQueue` are `Unpin`.
        //
        // I really hope that Rust can safely use self-reference types.
        #[inline]
        unsafe fn setup_queue(
            fd: &Fd,
            p: &sys::io_uring_params,
        ) -> io::Result<(MemoryMap, SubmissionQueue, CompletionQueue)> {
            let sq_len = p.sq_off.array as usize + p.sq_entries as usize * mem::size_of::<u32>();
            let cq_len = p.cq_off.cqes as usize
                + p.cq_entries as usize * mem::size_of::<sys::io_uring_cqe>();
            let sqe_len = p.sq_entries as usize * mem::size_of::<sys::io_uring_sqe>();
            let sqe_mmap = Mmap::new(fd, sys::IORING_OFF_SQES as _, sqe_len)?;

            if p.features & sys::IORING_FEAT_SINGLE_MMAP != 0 {
                let scq_mmap =
                    Mmap::new(fd, sys::IORING_OFF_SQ_RING as _, cmp::max(sq_len, cq_len))?;

                let sq = SubmissionQueue::new(&scq_mmap, &sqe_mmap, p);
                let cq = CompletionQueue::new(&scq_mmap, p);
                let mm = MemoryMap {
                    sq_mmap: scq_mmap,
                    cq_mmap: None,
                    sqe_mmap,
                };

                Ok((mm, sq, cq))
            } else {
                let sq_mmap = Mmap::new(fd, sys::IORING_OFF_SQ_RING as _, sq_len)?;
                let cq_mmap = Mmap::new(fd, sys::IORING_OFF_CQ_RING as _, cq_len)?;

                let sq = SubmissionQueue::new(&sq_mmap, &sqe_mmap, p);
                let cq = CompletionQueue::new(&cq_mmap, p);
                let mm = MemoryMap {
                    cq_mmap: Some(cq_mmap),
                    sq_mmap,
                    sqe_mmap,
                };

                Ok((mm, sq, cq))
            }
        }

        let fd: Fd = unsafe {
            sys::io_uring_setup(entries, &mut p)
                .try_into()
                .map_err(|_| io::Error::last_os_error())?
        };

        let (mm, sq, cq) = unsafe { setup_queue(&fd, &p)? };

        Ok(IoUring {
            inner: Inner {
                fd,
                params: Parameters(p),
                memory: ManuallyDrop::new(mm),
            },
            sq,
            cq
        })
    }

    /// Get the submitter of this io_uring instance, which can be used to submit submission queue
    /// events to the kernel for execution and to register files or buffers with it.
    #[inline]
    pub fn submitter(&self) -> Submitter<'_> {
        Submitter::new(
            &self.inner.fd,
            &self.inner.params,
            self.sq.head,
            self.sq.tail,
            self.sq.flags
        )
    }

    /// Get the parameters that were used to construct this instance.
    #[inline]
    pub fn params(&self) -> &Parameters {
        &self.inner.params
    }

    /// Initiate and/or complete asynchronous I/O. See [`Submitter::enter`] for more details.
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
        self.submitter().enter(to_submit, min_complete, flag, sig)
    }

    /// Initiate asynchronous I/O. See [`Submitter::submit`] for more details.
    #[inline]
    pub fn submit(&self) -> io::Result<usize> {
        self.submitter().submit()
    }

    /// Initiate and/or complete asynchronous I/O. See [`Submitter::submit_and_wait`] for more
    /// details.
    #[inline]
    pub fn submit_and_wait(&self, want: usize) -> io::Result<usize> {
        self.submitter().submit_and_wait(want)
    }

    /// Get the submitter, submission queue and completion queue of the io_uring instance. This can
    /// be used to operate on the different parts of the io_uring instance independently.
    pub fn split(&mut self) -> (Submitter<'_>, &mut SubmissionQueue, &mut CompletionQueue) {
        let submit = Submitter::new(
            &self.inner.fd,
            &self.inner.params,
            self.sq.head,
            self.sq.tail,
            self.sq.flags
        );
        (submit, &mut self.sq, &mut self.cq)
    }

    /// Get the submission queue of the io_uring instace. This is used to send I/O requests to the
    /// kernel.
    pub fn submission(&mut self) -> &mut SubmissionQueue {
        &mut self.sq
    }

    /// Get completion queue. This is used to receive I/O completion events from the kernel.
    pub fn completion(&mut self) -> &mut CompletionQueue {
        &mut self.cq
    }

    /// Make this `IoUring` instance concurrent.
    #[cfg(feature = "concurrent")]
    pub fn concurrent(self) -> concurrent::IoUring {
        concurrent::IoUring::new(self)
    }

    #[inline]
    #[cfg(feature = "unstable")]
    pub fn owned_split(self) -> (ownedsplit::SubmissionUring, ownedsplit::CompletionUring) {
        ownedsplit::split(self)
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Ensure that `MemoryMap` is released before `fd`.
        unsafe {
            ManuallyDrop::drop(&mut self.memory);
        }
    }
}

impl Builder {
    /// Do not make this io_uring instance accessible by child processes after a fork.
    pub fn dontfork(&mut self) -> &mut Self {
        self.dontfork = true;
        self
    }

    /// Perform busy-waiting for I/O completion events, as opposed to getting notifications via an
    /// asynchronous IRQ (Interrupt Request). This will reduce latency, but increases CPU usage.
    ///
    /// This is only usable on file systems that support polling and files opened with `O_DIRECT`.
    pub fn setup_iopoll(&mut self) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_IOPOLL;
        self
    }

    /// Use a kernel thread to perform submission queue polling. This allows your application to
    /// issue I/O without ever context switching into the kernel, however it does use up a lot more
    /// CPU. You should use it when you are expecting very large amounts of I/O.
    ///
    /// After `idle` seconds, the kernel thread will go to sleep and you will have to wake it up
    /// again with a system call (this is handled by [`Submitter::submit`] and
    /// [`Submitter::submit_and_wait`] automatically).
    ///
    /// When using this, you _must_ register all file descriptors with the [`Submitter`] via
    /// [`Submitter::register_files`].
    ///
    /// This requires root priviliges.
    pub fn setup_sqpoll(&mut self, idle: u32) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_SQPOLL;
        self.params.sq_thread_idle = idle;
        self
    }

    /// Bind the kernel's poll thread to the specified cpu. This flag is only meaningful when
    /// [`Builder::setup_sqpoll`] is enabled.
    pub fn setup_sqpoll_cpu(&mut self, cpu: u32) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_SQ_AFF;
        self.params.sq_thread_cpu = cpu;
        self
    }

    /// Create the completion queue with the specified number of entries. The value must be greater
    /// than `entries`, and may be rounded up to the next power-of-two.
    pub fn setup_cqsize(&mut self, entries: u32) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_CQSIZE;
        self.params.cq_entries = entries;
        self
    }

    /// Clamp the sizes of the submission queue and completion queue at their maximum values instead
    /// of returning an error when you attempt to resize them beyond their maximum values.
    pub fn setup_clamp(&mut self) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_CLAMP;
        self
    }

    /// Share the asynchronous worker thread backend of this io_uring with the specified io_uring
    /// file descriptor instead of creating a new thread pool.
    pub fn setup_attach_wq(&mut self, fd: RawFd) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_ATTACH_WQ;
        self.params.wq_fd = fd as _;
        self
    }

    /// Start the io_uring instance with all its rings disabled. This allows you to register
    /// restrictions, buffers and files before the kernel starts processing submission queue
    /// events. You are only able to [register restrictions](Submitter::register_restrictions) when
    /// the rings are disabled due to concurrency issues. You can enable the rings with
    /// [`Submitter::register_enable_rings`].
    ///
    /// Requires the `unstable` feature.
    #[cfg(feature = "unstable")]
    pub fn setup_r_disabled(&mut self) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_R_DISABLED;
        self
    }

    /// Build an [IoUring], with the specified number of entries in the submission queue and
    /// completion queue unless [`setup_cqsize`](Self::setup_cqsize) has been called.
    pub fn build(&self, entries: u32) -> io::Result<IoUring> {
        let ring = IoUring::with_params(entries, self.params)?;

        if self.dontfork {
            ring.inner.memory.sq_mmap.dontfork()?;
            ring.inner.memory.sqe_mmap.dontfork()?;
            if let Some(cq_mmap) = ring.inner.memory.cq_mmap.as_ref() {
                cq_mmap.dontfork()?;
            }
        }

        Ok(ring)
    }
}

impl Parameters {
    /// Whether a kernel thread is performing queue polling. Enabled with [`Builder::setup_sqpoll`].
    pub fn is_setup_sqpoll(&self) -> bool {
        self.0.flags & sys::IORING_SETUP_SQPOLL != 0
    }

    /// Whether waiting for completion events is done with a busy loop instead of using IRQs.
    /// Enabled with [`Builder::setup_iopoll`].
    pub fn is_setup_iopoll(&self) -> bool {
        self.0.flags & sys::IORING_SETUP_IOPOLL != 0
    }

    /// If this flag is set, the SQ and CQ rings were mapped with a single `mmap(2)` call. This
    /// means that only two syscalls were used instead of three.
    pub fn is_feature_single_mmap(&self) -> bool {
        self.0.features & sys::IORING_FEAT_SINGLE_MMAP != 0
    }

    /// If this flag is set, io_uring supports never dropping completion events. If a completion
    /// event occurs and the CQ ring is full, the kernel stores the event internally until such a
    /// time that the CQ ring has room for more entries.
    pub fn is_feature_nodrop(&self) -> bool {
        self.0.features & sys::IORING_FEAT_NODROP != 0
    }

    /// If this flag is set, applications can be certain that any data for async offload has been
    /// consumed when the kernel has consumed the SQE.
    pub fn is_feature_submit_stable(&self) -> bool {
        self.0.features & sys::IORING_FEAT_SUBMIT_STABLE != 0
    }

    /// If this flag is set, applications can specify offset == -1 with [`Readv`](opcode::Readv),
    /// [`Writev`](opcode::Writev), [`ReadFixed`](opcode::ReadFixed),
    /// [`WriteFixed`](opcode::WriteFixed), [`Read`](opcode::Read) and [`Write`](opcode::Write),
    /// which behaves exactly like setting offset == -1 in `preadv2(2)` and `pwritev2(2)`: it’ll use
    /// (and update) the current file position.
    ///
    /// This obviously comes with the caveat that if the application has multiple reads or writes in flight,
    /// then the end result will not be as expected.
    /// This is similar to threads sharing a file descriptor and doing IO using the current file position.
    pub fn is_feature_rw_cur_pos(&self) -> bool {
        self.0.features & sys::IORING_FEAT_RW_CUR_POS != 0
    }

    /// If this flag is set, then io_uring guarantees that both sync and async execution of
    /// a request assumes the credentials of the task that called [`Submitter::enter`] to queue the requests.
    /// If this flag isn’t set, then requests are issued with the credentials of the task that originally registered the io_uring.
    /// If only one task is using a ring, then this flag doesn’t matter as the credentials will always be the same.
    ///
    /// Note that this is the default behavior, tasks can still register different personalities
    /// through [`Submitter::register_personality`].
    pub fn is_feature_cur_personality(&self) -> bool {
        self.0.features & sys::IORING_FEAT_CUR_PERSONALITY != 0
    }

    /// Whether async pollable I/O is fast.
    ///
    /// See [the commit message that introduced
    /// it](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/commit/?id=d7718a9d25a61442da8ee8aeeff6a0097f0ccfd6)
    /// for more details.
    ///
    /// Requires the `unstable` feature.
    #[cfg(feature = "unstable")]
    pub fn is_feature_fast_poll(&self) -> bool {
        self.0.features & sys::IORING_FEAT_FAST_POLL != 0
    }

    /// Whether poll events are stored using 32 bits instead of 16. This allows the user to use
    /// `EPOLLEXCLUSIVE`.
    #[cfg(feature = "unstable")]
    pub fn is_feature_poll_32bits(&self) -> bool {
        self.0.features & sys::IORING_FEAT_POLL_32BITS != 0
    }

    #[cfg(feature = "unstable")]
    pub fn is_feature_sqpoll_nonfixed(&self) -> bool {
        self.0.features & sys::IORING_FEAT_SQPOLL_NONFIXED != 0
    }

    #[cfg(feature = "unstable")]
    pub fn is_feature_ext_arg(&self) -> bool {
        self.0.features & sys::IORING_FEAT_EXT_ARG != 0
    }

    /// The number of submission queue entries allocated.
    pub fn sq_entries(&self) -> u32 {
        self.0.sq_entries
    }

    /// The number of completion queue entries allocated.
    pub fn cq_entries(&self) -> u32 {
        self.0.cq_entries
    }
}

impl AsRawFd for IoUring {
    fn as_raw_fd(&self) -> RawFd {
        self.inner.fd.as_raw_fd()
    }
}
