use std::os::unix::io::AsRawFd;
use std::sync::atomic;
use std::{io, ptr};

/// A region of memory mapped using `mmap(2)`.
pub struct Mmap {
    addr: ptr::NonNull<libc::c_void>,
    len: usize,
}

impl Mmap {
    /// Map `len` bytes starting from the offset `offset` in the file descriptor `fd` into memory.
    pub fn new(fd: &Fd, offset: libc::off_t, len: usize) -> io::Result<Mmap> {
        unsafe {
            match libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_POPULATE,
                fd.as_raw_fd(),
                offset,
            ) {
                libc::MAP_FAILED => Err(io::Error::last_os_error()),
                addr => {
                    // here, `mmap` will never return null
                    let addr = ptr::NonNull::new_unchecked(addr);
                    Ok(Mmap { addr, len })
                }
            }
        }
    }

    /// Do not make the stored memory accessible by child processes after a `fork`.
    pub fn dontfork(&self) -> io::Result<()> {
        match unsafe { libc::madvise(self.addr.as_ptr(), self.len, libc::MADV_DONTFORK) } {
            0 => Ok(()),
            _ => Err(io::Error::last_os_error()),
        }
    }

    /// Get a pointer to the memory.
    #[inline]
    pub fn as_mut_ptr(&self) -> *mut libc::c_void {
        self.addr.as_ptr()
    }

    /// Get a pointer to the data at the given offset.
    #[inline]
    pub unsafe fn offset(&self, offset: u32) -> *mut libc::c_void {
        self.as_mut_ptr().add(offset as usize)
    }
}

impl Drop for Mmap {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.addr.as_ptr(), self.len);
        }
    }
}

pub use fd::Fd;

#[cfg(feature = "io_safety")]
mod fd {
    use std::os::unix::io::{AsRawFd, RawFd, IntoRawFd, FromRawFd, OwnedFd, AsFd, BorrowedFd};

    /// An owned file descriptor.
    pub struct Fd(pub OwnedFd);

    impl Fd {
        /// Try to convert from a `RawFd` to an `Fd`.
        /// 
        /// # Safety
        /// 
        /// Must be a valid file descriptor.
        pub unsafe fn from_raw(value: RawFd) -> Option<Fd> {
            if value >= 0 {
                // SAFETY: not -1, and this is a valid FD
                Some(Fd(OwnedFd::from_raw_fd(value)))
            } else {
                None
            }
        }       
    }

    impl AsRawFd for Fd {
        fn as_raw_fd(&self) -> RawFd {
            self.0.as_raw_fd()
        }
    }

    impl IntoRawFd for Fd {
        fn into_raw_fd(self) -> RawFd {
            self.0.into_raw_fd()
        }
    }

    impl FromRawFd for Fd {
        unsafe fn from_raw_fd(fd: RawFd) -> Self {
            Fd(OwnedFd::from_raw_fd(fd))
        }
    }

    impl AsFd for Fd {
        fn as_fd(&self) -> BorrowedFd<'_> {
            self.0.as_fd()
        }
    }

    impl From<OwnedFd> for Fd {
        fn from(fd: OwnedFd) -> Self {
            Fd(fd)
        }
    }

    impl From<Fd> for OwnedFd {
        fn from(fd: Fd) -> Self {
            fd.0
        }
    }
}

#[cfg(not(feature = "io_safety"))]
mod fd {
    use std::mem;
    use std::os::unix::io::{AsRawFd, RawFd, IntoRawFd, FromRawFd};

    /// An owned file descriptor.
    pub struct Fd(pub RawFd);

    impl Fd {
        /// Try to convert from a `RawFd` to an `Fd`.
        /// 
        /// # Safety
        /// 
        /// Must be a valid file descriptor.
        pub unsafe fn from_raw(value: RawFd) -> Option<Fd> {
            if value >= 0 {
                Some(Fd(value))
            } else {
                None
            }
        }
    }

    impl AsRawFd for Fd {
        #[inline]
        fn as_raw_fd(&self) -> RawFd {
            self.0
        }
    }

    impl IntoRawFd for Fd {
        #[inline]
        fn into_raw_fd(self) -> RawFd {
            let fd = self.0;
            mem::forget(self);
            fd
        }
    }

    impl FromRawFd for Fd {
        #[inline]
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
}

#[inline(always)]
pub unsafe fn unsync_load(u: *const atomic::AtomicU32) -> u32 {
    *u.cast::<u32>()
}

#[inline]
pub const fn cast_ptr<T>(n: &T) -> *const T {
    n
}
