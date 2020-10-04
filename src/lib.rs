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
    fd: Fd,
    params: Parameters,
    memory: ManuallyDrop<MemoryMap>,
    sq: SubmissionQueue,
    cq: CompletionQueue,
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

#[derive(Clone)]
pub struct Parameters(sys::io_uring_params);

unsafe impl Send for IoUring {}
unsafe impl Sync for IoUring {}

impl IoUring {
    /// Create a IoUring instance
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
            fd,
            sq,
            cq,
            params: Parameters(p),
            memory: ManuallyDrop::new(mm),
        })
    }

    #[inline]
    pub fn submitter(&self) -> Submitter<'_> {
        Submitter::new(&self.fd, self.params.0.flags, &self.sq)
    }

    #[inline]
    pub fn params(&self) -> &Parameters {
        &self.params
    }

    /// Initiate and/or complete asynchronous I/O
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

    /// Initiate asynchronous I/O.
    #[inline]
    pub fn submit(&self) -> io::Result<usize> {
        self.submitter().submit()
    }

    /// Initiate and/or complete asynchronous I/O
    #[inline]
    pub fn submit_and_wait(&self, want: usize) -> io::Result<usize> {
        self.submitter().submit_and_wait(want)
    }

    /// Get submitter and submission queue and completion queue
    pub fn split(&mut self) -> (Submitter<'_>, &mut SubmissionQueue, &mut CompletionQueue) {
        let submit = Submitter::new(&self.fd, self.params.0.flags, &self.sq);
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
    pub fn dontfork(&mut self) -> &mut Self {
        self.dontfork = true;
        self
    }

    /// Perform busy-waiting for an I/O completion,
    /// as opposed to getting notifications via an asynchronous IRQ (Interrupt Request).
    pub fn setup_iopoll(&mut self) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_IOPOLL;
        self
    }

    /// When this flag is specified, a kernel thread is created to perform submission queue polling.
    /// An io_uring instance configured in this way enables an application to issue I/O
    /// without ever context switching into the kernel.
    pub fn setup_sqpoll(&mut self, idle: impl Into<Option<u32>>) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_SQPOLL;
        self.params.sq_thread_idle = idle.into().unwrap_or(0);
        self
    }

    /// If this flag is specified,
    /// then the poll thread will be bound to the cpu set in the value.
    /// This flag is only meaningful when [Builder::setup_sqpoll] is enabled.
    pub fn setup_sqpoll_cpu(&mut self, n: u32) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_SQ_AFF;
        self.params.sq_thread_cpu = n;
        self
    }

    /// Create the completion queue with struct `io_uring_params.cq_entries` entries.
    /// The value must be greater than entries, and may be rounded up to the next power-of-two.
    pub fn setup_cqsize(&mut self, n: u32) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_CQSIZE;
        self.params.cq_entries = n;
        self
    }

    pub fn setup_clamp(&mut self) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_CLAMP;
        self
    }

    pub fn setup_attach_wq(&mut self, fd: RawFd) -> &mut Self {
        self.params.flags |= sys::IORING_SETUP_ATTACH_WQ;
        self.params.wq_fd = fd as _;
        self
    }

    /// Build a [IoUring].
    #[inline]
    pub fn build(&self, entries: u32) -> io::Result<IoUring> {
        let ring = IoUring::with_params(entries, self.params)?;

        if self.dontfork {
            ring.memory.sq_mmap.dontfork()?;
            ring.memory.sqe_mmap.dontfork()?;
            if let Some(cq_mmap) = ring.memory.cq_mmap.as_ref() {
                cq_mmap.dontfork()?;
            }
        }

        Ok(ring)
    }
}

impl Parameters {
    pub fn is_setup_sqpoll(&self) -> bool {
        self.0.flags & sys::IORING_SETUP_SQPOLL != 0
    }

    pub fn is_setup_iopoll(&self) -> bool {
        self.0.flags & sys::IORING_SETUP_IOPOLL != 0
    }

    /// If this flag is set, the two SQ and CQ rings can be mapped with a single `mmap(2)` call.
    /// The SQEs must still be allocated separately.
    /// This brings the necessary `mmap(2)` calls down from three to two.
    pub fn is_feature_single_mmap(&self) -> bool {
        self.0.features & sys::IORING_FEAT_SINGLE_MMAP != 0
    }

    /// If this flag is set, io_uring supports never dropping completion events. If a completion
    /// event occurs and the CQ ring is full, the kernel stores the event internally until such a
    /// time that the CQ ring has room for more entries.
    pub fn is_feature_nodrop(&self) -> bool {
        self.0.features & sys::IORING_FEAT_NODROP != 0
    }

    /// If this flag is set, applications can be certain that any data for async offload has been consumed
    /// when the kernel has consumed the SQE
    pub fn is_feature_submit_stable(&self) -> bool {
        self.0.features & sys::IORING_FEAT_SUBMIT_STABLE != 0
    }

    /// If this flag is set, applications can specify offset == -1 with
    /// `IORING_OP_{READV,WRITEV}`, `IORING_OP_{READ,WRITE}_FIXED`, and `IORING_OP_{READ,WRITE}`
    /// to mean current file position, which behaves like `preadv2(2)` and `pwritev2(2)` with offset == -1.
    /// It’ll use (and update) the current file position.
    ///
    /// This obviously comes with the caveat that if the application has multiple reads or writes in flight,
    /// then the end result will not be as expected.
    /// This is similar to threads sharing a file descriptor and doing IO using the current file position.
    pub fn is_feature_rw_cur_pos(&self) -> bool {
        self.0.features & sys::IORING_FEAT_RW_CUR_POS != 0
    }

    /// If this flag is set, then io_uring guarantees that both sync and async execution of
    /// a request assumes the credentials of the task that called `io_uring_enter(2)` to queue the requests.
    /// If this flag isn’t set, then requests are issued with the credentials of the task that originally registered the io_uring.
    /// If only one task is using a ring, then this flag doesn’t matter as the credentials will always be the same.
    /// Note that this is the default behavior,
    /// tasks can still register different personalities through
    /// `io_uring_register(2)` with `IORING_REGISTER_PERSONALITY` and specify the personality to use in the sqe.
    pub fn is_feature_cur_personality(&self) -> bool {
        self.0.features & sys::IORING_FEAT_CUR_PERSONALITY != 0
    }

    #[cfg(feature = "unstable")]
    pub fn is_feature_fast_poll(&self) -> bool {
        self.0.features & sys::IORING_FEAT_FAST_POLL != 0
    }

    pub fn sq_entries(&self) -> u32 {
        self.0.sq_entries
    }

    pub fn cq_entries(&self) -> u32 {
        self.0.cq_entries
    }
}

impl AsRawFd for IoUring {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}
