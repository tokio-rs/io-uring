mod util;
mod register;
pub mod squeue;
pub mod cqueue;
pub mod opcode;

use std::{ ptr, cmp };
use std::convert::TryInto;
use std::os::unix::io::AsRawFd;
use std::io::{ self, IoSlice };
use std::mem::ManuallyDrop;
use linux_io_uring_sys as sys;
use util::Fd;
use squeue::SubmissionQueue;
use cqueue::CompletionQueue;
use register::{ register as reg, unregister as unreg };


pub struct IoUring {
    fd: Fd,
    sq: ManuallyDrop<SubmissionQueue>,
    cq: ManuallyDrop<CompletionQueue>
}

impl IoUring {
    pub fn new(entries: u32) -> io::Result<IoUring> {
        let mut p = sys::io_uring_params::default();

        // TODO flags

        let fd: Fd = unsafe {
            sys::io_uring_setup(entries, &mut p)
                .try_into()
                .map_err(|_| io::Error::last_os_error())?
        };

        let sq = ManuallyDrop::new(SubmissionQueue::new(&fd, &p)?);
        let cq = ManuallyDrop::new(CompletionQueue::new(&fd, &p)?);

        Ok(IoUring { fd, sq, cq })
    }

    pub unsafe fn register(&self, target: reg::Target<'_, '_>) -> io::Result<()> {
        let (opcode, arg, len) = target.export();

        unsafe {
            if 0 == sys::io_uring_register(self.fd.as_raw_fd(), opcode, arg, len) {
               Ok(())
            } else {
               Err(io::Error::last_os_error())
            }
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
        -> io::Result<()>
    {
        let sig = sig.map(|sig| sig as *const _).unwrap_or_else(ptr::null);
        if 0 == sys::io_uring_enter(self.fd.as_raw_fd(), to_submit, min_complete, flag, sig) {
            Ok(())
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

        unsafe {
            self.enter(len as _, want as _, 0, None)?;
        }

        Ok(len)
    }

    pub fn submission(&mut self) -> &mut SubmissionQueue {
        &mut self.sq
    }
}

impl Drop for IoUring {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.sq);
            ManuallyDrop::drop(&mut self.cq);
        }
    }
}
