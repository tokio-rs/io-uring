mod util;
mod register;
pub mod squeue;
pub mod cqueue;

use std::ptr;
use std::convert::TryInto;
use std::os::unix::io::AsRawFd;
use std::io::{ self, IoSlice };
use std::mem::{ MaybeUninit, ManuallyDrop };
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
        let mut p = MaybeUninit::<sys::io_uring_params>::zeroed();

        // TODO flags

        let fd: Fd = unsafe {
            sys::io_uring_setup(entries, p.as_mut_ptr())
                .try_into()
                .map_err(|_| io::Error::last_os_error())?
        };

        let p = unsafe { p.assume_init() };

        let sq = ManuallyDrop::new(SubmissionQueue::new(&fd, &p)?);
        let cq = ManuallyDrop::new(CompletionQueue::new(&fd, &p)?);

        Ok(IoUring { fd, sq, cq })
    }

    pub fn register(&self, target: reg::Target<'_, '_>) -> io::Result<()> {
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

    pub fn submission() {
        unimplemented!()
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
