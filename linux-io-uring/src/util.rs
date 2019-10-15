use std::{ io, ptr, mem };
use std::ops::Deref;
use std::sync::atomic;
use std::convert::TryFrom;
use std::os::unix::io::{ AsRawFd, IntoRawFd, FromRawFd, RawFd };


#[macro_export]
macro_rules! mmap_offset {
    ( $mmap:ident + $offset:expr => $ty:ty ) => {
        $mmap.as_mut_ptr().add($offset as _) as $ty
    };
    ( unsafe $( let $val:ident = $mmap:ident + $offset:expr => $ty:ty );+ $(;)? ) => {
        $(
            let $val = unsafe { mmap_offset!($mmap + $offset => $ty) };
        )*
    }
}


pub struct Mmap {
    addr: *mut libc::c_void,
    len: usize
}

impl Mmap {
    pub fn new(fd: &Fd, offset: i64, len: usize) -> io::Result<Mmap> {
        unsafe {
            match libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_POPULATE,
                fd.as_raw_fd(),
                offset
            ) {
                libc::MAP_FAILED => Err(io::Error::last_os_error()),
                addr => Ok(Mmap { addr, len })
            }
        }
    }

    pub fn as_mut_ptr(&self) -> *mut libc::c_void {
        self.addr
    }
}

impl Drop for Mmap {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.addr, self.len);

            // TODO log fail
        }
    }
}

pub struct Fd(RawFd);

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

pub struct AtomicU32Ref(*const u32);

impl AtomicU32Ref {
    pub unsafe fn new(v: *const u32) -> AtomicU32Ref {
        AtomicU32Ref(v)
    }

    pub fn unsync_load(&self) -> u32 {
        unsafe { *self.0 }
    }
}

impl Deref for AtomicU32Ref {
    type Target = atomic::AtomicU32;

    fn deref(&self) -> &Self::Target {
        unsafe { &*(self.0 as *const atomic::AtomicU32) }
    }
}
