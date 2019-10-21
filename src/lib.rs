mod util;
mod register;
pub mod squeue;
pub mod cqueue;
pub mod opcode;
pub mod concurrent;

use std::{ io, ptr, cmp };
use std::convert::TryInto;
use std::os::unix::io::AsRawFd;
use std::mem::ManuallyDrop;
use bitflags::bitflags;
use linux_io_uring_sys as sys;
use util::Fd;
use squeue::SubmissionQueue;
use cqueue::CompletionQueue;
pub use register::{ register as reg, unregister as unreg };


pub struct IoUring {
    fd: Fd,
    flags: SetupFlags,
    sq: ManuallyDrop<SubmissionQueue>,
    cq: ManuallyDrop<CompletionQueue>
}

bitflags!{
    pub struct SetupFlags: u32 {
        const IOPOLL = sys::IORING_SETUP_IOPOLL;
        const SQPOLL = sys::IORING_SETUP_SQPOLL;
        const SQ_AFF = sys::IORING_SETUP_SQ_AFF;
    }
}

#[derive(Clone)]
pub struct Builder {
    entries: u32,
    params: sys::io_uring_params
}

impl IoUring {
    #[inline]
    pub fn new(entries: u32) -> io::Result<IoUring> {
        IoUring::with_params(entries, sys::io_uring_params::default())
    }

    pub fn with_params(entries: u32, mut p: sys::io_uring_params) -> io::Result<IoUring> {
        #[inline]
        fn setup_queue(fd: &Fd, p: &sys::io_uring_params)
            -> io::Result<(SubmissionQueue, CompletionQueue)>
        {
            Ok((SubmissionQueue::new(fd, p)?, CompletionQueue::new(fd, p)?))
        }

        let fd: Fd = unsafe {
            sys::io_uring_setup(entries, &mut p)
                .try_into()
                .map_err(|_| io::Error::last_os_error())?
        };

        let flags = SetupFlags::from_bits_truncate(p.flags);
        let (sq, cq) = setup_queue(&fd, &p)?;

        Ok(IoUring {
            fd, flags,
            sq: ManuallyDrop::new(sq),
            cq: ManuallyDrop::new(cq)
        })
    }

    pub unsafe fn register(&self, target: reg::Target<'_, '_>) -> io::Result<()> {
        let (opcode, arg, len) = target.export();

        if 0 == sys::io_uring_register(self.fd.as_raw_fd(), opcode, arg, len) {
           Ok(())
        } else {
           Err(io::Error::last_os_error())
        }
    }

    pub fn unregister(&self, target: unreg::Target) -> io::Result<()> {
        let opcode = target.opcode();

        unsafe {
             if 0 == sys::io_uring_register(self.fd.as_raw_fd(), opcode, ptr::null(), 0) {
                Ok(())
             } else {
                Err(io::Error::last_os_error())
             }
        }
    }

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

    pub fn submit(&self) -> io::Result<usize> {
        self.submit_and_wait(0)
    }

    pub fn submit_and_wait(&self, want: usize) -> io::Result<usize> {
        let len = self.sq.len();
        let want = cmp::min(len, want);

        let flags = match want {
            0 if self.flags.contains(SetupFlags::SQPOLL) =>
                if self.sq.need_wakeup() {
                    sys::IORING_ENTER_SQ_WAKEUP
                } else {
                    // fast poll
                    return Ok(len)
                },
            0 => 0,
            _ => sys::IORING_ENTER_GETEVENTS,
        };

        unsafe {
            self.enter(len as _, want as _, flags, None)
        }
    }

    pub fn submission_and_completion(&mut self)
        -> (&mut SubmissionQueue, &mut CompletionQueue)
    {
        (&mut self.sq, &mut self.cq)
    }

    pub fn submission(&mut self) -> &mut SubmissionQueue {
        &mut self.sq
    }

    pub fn completion(&mut self) -> &mut CompletionQueue {
        &mut self.cq
    }

    pub fn concurrent(self) -> concurrent::IoUring {
        concurrent::IoUring::new(self)
    }
}

unsafe impl Send for IoUring {}
unsafe impl Sync for IoUring {}

impl Drop for IoUring {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.sq);
            ManuallyDrop::drop(&mut self.cq);
        }
    }
}

impl Builder {
    pub fn new(entries: u32) -> Self {
        Builder {
            entries,
            params: sys::io_uring_params::default()
        }
    }

    pub fn setup_flags(mut self, flags: SetupFlags) -> Self {
        self.params.flags = flags.bits();
        self
    }

    pub fn thread_cpu(mut self, n: u32) -> Self {
        self.params.sq_thread_cpu = n;
        self
    }

    pub fn thread_idle(mut self, n: u32) -> Self {
        self.params.sq_thread_idle = n;
        self
    }

    pub fn build(self) -> io::Result<IoUring> {
        IoUring::with_params(self.entries, self.params)
    }
}
