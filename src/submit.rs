use std::{ io, ptr };
use std::sync::atomic;
use std::os::unix::io::{ AsRawFd, RawFd };
use crate::register::{ register as reg, unregister as unreg, execute };
use crate::squeue::SubmissionQueue;
use crate::util::{ Fd, unsync_load };
use crate::register::Probe;
use crate::sys;


/// Submitter
pub struct Submitter<'a> {
    fd: &'a Fd,
    flags: u32,

    sq_head: *const atomic::AtomicU32,
    sq_tail: *const atomic::AtomicU32,
    sq_flags: *const atomic::AtomicU32
}

impl<'a> Submitter<'a> {
    #[inline]
    pub(crate) const fn new(fd: &'a Fd, flags: u32, sq: &SubmissionQueue) -> Submitter<'a> {
        Submitter {
            fd, flags,
            sq_head: sq.head,
            sq_tail: sq.tail,
            sq_flags: sq.flags
        }
    }

    fn sq_len(&self) -> usize {
        unsafe {
            let head = (*self.sq_head).load(atomic::Ordering::Acquire);
            let tail = unsync_load(self.sq_tail);

            tail.wrapping_sub(head) as usize
        }
    }

    fn sq_need_wakeup(&self) -> bool {
        unsafe {
            (*self.sq_flags).load(atomic::Ordering::Acquire) & sys::IORING_SQ_NEED_WAKEUP
                != 0
        }
    }

    /// Register files or user buffers for asynchronous I/O.
    #[inline]
    #[deprecated(since="0.3.5", note="please use `Submitter::register_*` instead")]
    pub fn register(&self, target: reg::Target<'_>) -> io::Result<()> {
        target.execute(self.fd.as_raw_fd())
    }

    /// Unregister files or user buffers for asynchronous I/O.
    #[inline]
    #[deprecated(since="0.3.5", note="please use `Submitter::unregister_*` instead")]
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

    pub fn register_buffers(&self, bufs: &[libc::iovec]) -> io::Result<()> {
        execute(self.fd.as_raw_fd(), sys::IORING_REGISTER_BUFFERS, bufs.as_ptr() as *const _, bufs.len() as _)?;
        Ok(())
    }

    pub fn register_files(&self, fds: &[RawFd]) -> io::Result<()> {
        execute(self.fd.as_raw_fd(), sys::IORING_REGISTER_FILES, fds.as_ptr() as *const _, fds.len() as _)?;
        Ok(())
    }

    pub fn register_eventfd(&self, eventfd: RawFd) -> io::Result<()> {
        execute(self.fd.as_raw_fd(), sys::IORING_REGISTER_EVENTFD, cast_ptr::<RawFd>(&eventfd) as *const _, 1)?;
        Ok(())
    }

    pub fn register_files_update(&self, offset: u32, fds: &[RawFd]) -> io::Result<usize> {
        let fu = sys::io_uring_files_update {
            offset,
            resv: 0,
            fds: fds.as_ptr() as _
        };
        let fu = cast_ptr::<sys::io_uring_files_update>(&fu);
        let ret = execute(self.fd.as_raw_fd(), sys::IORING_REGISTER_FILES_UPDATE, fu as *const _, fds.len() as _)?;
        Ok(ret as _)
    }

    pub fn register_eventfd_async(&self, eventfd: RawFd) -> io::Result<()> {
        execute(self.fd.as_raw_fd(), sys::IORING_REGISTER_EVENTFD_ASYNC, cast_ptr::<RawFd>(&eventfd) as *const _, 1)?;
        Ok(())
    }

    pub fn register_probe(&self, probe: &mut Probe) -> io::Result<()> {
        execute(self.fd.as_raw_fd(), sys::IORING_REGISTER_PROBE, probe.as_mut_ptr() as *const _, 256)?;
        Ok(())
    }

    pub fn register_personality(&self) -> io::Result<()> {
        execute(self.fd.as_raw_fd(), sys::IORING_REGISTER_PERSONALITY, ptr::null(), 0)?;
        Ok(())
    }

    pub fn unregister_buffers(&self) -> io::Result<()> {
        execute(self.fd.as_raw_fd(), sys::IORING_UNREGISTER_BUFFERS, ptr::null(), 0)?;
        Ok(())
    }

    pub fn unregister_files(&self) -> io::Result<()> {
        execute(self.fd.as_raw_fd(), sys::IORING_UNREGISTER_FILES, ptr::null(), 0)?;
        Ok(())
    }

    pub fn unregister_eventfd(&self) -> io::Result<()> {
        execute(self.fd.as_raw_fd(), sys::IORING_UNREGISTER_EVENTFD, ptr::null(), 0)?;
        Ok(())
    }

    pub fn unregister_personality(&self) -> io::Result<()> {
        execute(self.fd.as_raw_fd(), sys::IORING_UNREGISTER_PERSONALITY, ptr::null(), 0)?;
        Ok(())
    }
}

#[inline]
fn cast_ptr<T>(n: &T) -> *const T {
    n as *const T
}
