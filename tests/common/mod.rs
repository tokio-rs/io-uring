#![allow(dead_code)]

use std::mem;
use std::convert::TryFrom;
use std::os::unix::io::{ AsRawFd, IntoRawFd, FromRawFd, RawFd };
use linux_io_uring::opcode::{ self, types };


pub struct Fd(pub RawFd);

impl TryFrom<RawFd> for Fd {
    type Error = ();

    fn try_from(value: RawFd) -> Result<Fd, Self::Error> {
        if value >= 0 {
            Ok(Fd(value))
        } else {
            Err(())
        }
    }
}

impl AsRawFd for Fd {
    fn as_raw_fd(&self) -> RawFd {
        self.0
    }
}

impl IntoRawFd for Fd {
    fn into_raw_fd(self) -> RawFd {
        let fd = self.0;
        mem::forget(self);
        fd
    }
}

impl FromRawFd for Fd {
    unsafe fn from_raw_fd(fd: RawFd) -> Fd {
        Fd(fd)
    }
}

impl Drop for Fd {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.0);
        }
    }
}

pub fn one_s() -> opcode::Timeout {
    static ONE_S: types::Timespec = types::Timespec { tv_sec: 1, tv_nsec: 0 };

    opcode::Timeout::new(&ONE_S)
}
