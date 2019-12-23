use std::{ io, ptr };
use std::sync::atomic;
use std::os::unix::io::AsRawFd;
use linux_io_uring_sys as sys;
use crate::register::{ register as reg, unregister as unreg };
use crate::squeue::SubmissionQueue;
use crate::util::{ Fd, unsync_load };


/// Submitter
pub struct Submitter<'a> {
    fd: &'a Fd,
    flags: u32,

    sq_head: *const atomic::AtomicU32,
    sq_tail: *const atomic::AtomicU32,
    sq_flags: *const atomic::AtomicU32
}

impl<'a> Submitter<'a> {
    pub(crate) const fn new(fd: &'a Fd, flags: u32, sq: &SubmissionQueue) -> Submitter<'a> {
        Submitter {
            fd, flags,
            sq_head: sq.head,
            sq_tail: sq.tail,
            sq_flags: sq.flags
        }
    }

    fn sq_len(&self) -> usize {
        let head = unsafe { (*self.sq_head).load(atomic::Ordering::Acquire) };
        let tail = unsafe { unsync_load(self.sq_tail) };

        tail.wrapping_sub(head) as usize
    }

    fn sq_need_wakeup(&self) -> bool {
        unsafe {
            (*self.sq_flags).load(atomic::Ordering::Acquire) & sys::IORING_SQ_NEED_WAKEUP
                != 0
        }
    }

    /// Register files or user buffers for asynchronous I/O.
    #[inline]
    pub fn register(&self, target: reg::Target<'_>) -> io::Result<()> {
        target.execute(self.fd.as_raw_fd())
    }

    /// Unregister files or user buffers for asynchronous I/O.
    #[inline]
    pub fn unregister(&self, target: unreg::Target) -> io::Result<()> {
        target.execute(self.fd.as_raw_fd())
    }

    /// Initiate and/or complete asynchronous I/O
    ///
    /// # Safety
    ///
    /// This provides a raw interface so developer must ensure that parameters are correct.
    pub unsafe fn enter(&self, to_submit: u32, min_complete: u32, flag: u32, sig: Option<&libc::sigset_t>)
        -> io::Result<usize>
    {
        let sig = sig.map(|sig| sig as *const _).unwrap_or_else(ptr::null);
        let result = sys::io_uring_enter(self.fd.as_raw_fd(), to_submit, min_complete, flag, sig);
        if result >= 0 {
            Ok(result as _)
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Initiate asynchronous I/O.
    #[inline]
    pub fn submit(&self) -> io::Result<usize> {
        self.submit_and_wait(0)
    }

    /// Initiate and/or complete asynchronous I/O
    pub fn submit_and_wait(&self, want: usize) -> io::Result<usize> {
        let len = self.sq_len();

        let mut flags = 0;

        if want > 0 {
            flags |= sys::IORING_ENTER_GETEVENTS;
        }

        if self.flags & sys::IORING_SETUP_SQPOLL != 0 {
            if self.sq_need_wakeup() {
                flags |= sys::IORING_ENTER_SQ_WAKEUP;
            } else if want == 0 {
                // fast poll
                return Ok(len)
            }
        }

        unsafe {
            self.enter(len as _, want as _, flags, None)
        }
    }
}
