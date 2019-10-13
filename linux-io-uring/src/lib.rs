mod util;
mod queue;

use std::io;
use std::mem::{ MaybeUninit, ManuallyDrop };
use std::convert::TryInto;
use std::os::unix::io::RawFd;
use linux_io_uring_sys as sys;
use util::Fd;
use queue::{ SubmissionQueue, CompletionQueue };


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
                .try_into()?
        };

        let p = unsafe { p.assume_init() };

        let sq = ManuallyDrop::new(SubmissionQueue::new(&fd, &p)?);
        let cq = ManuallyDrop::new(CompletionQueue::new(&fd, &p)?);

        Ok(IoUring { fd, sq, cq })
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
