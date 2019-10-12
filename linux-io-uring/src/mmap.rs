use std::{ fs, io, ptr };
use std::os::unix::io::RawFd;


pub struct Mmap {
    addr: *mut libc::c_void,
    len: usize
}

impl Mmap {
    pub fn map(fd: RawFd, offset: i64, len: usize) -> io::Result<Mmap> {
        unsafe {
            match libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_POPULATE,
                fd,
                offset
            ) {
                libc::MAP_FAILED => return Err(io::Error::last_os_error()),
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
            if self.len != 0 {
                libc::munmap(self.addr, self.len);

                // TODO log fail
            }
        }
    }
}
