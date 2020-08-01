use std::{ io, ptr, mem };
use std::os::unix::io::RawFd;
use crate::sys;


pub(crate) fn execute(fd: RawFd, opcode: libc::c_uint, arg: *const libc::c_void, len: libc::c_uint)
    -> io::Result<i32>
{
    unsafe {
        let ret = sys::io_uring_register(fd, opcode, arg, len);
        if ret >= 0 {
            Ok(ret)
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

pub struct Probe(ptr::NonNull<sys::io_uring_probe>);

impl Probe {
    pub(crate) const COUNT: usize = 256;
    pub(crate) const SIZE: usize = mem::size_of::<sys::io_uring_probe>()
        + Self::COUNT * mem::size_of::<sys::io_uring_probe_op>();

    #[allow(clippy::cast_ptr_alignment)]
    pub fn new() -> Probe {
        use std::alloc::{ alloc_zeroed, Layout };

        let probe_align = Layout::new::<sys::io_uring_probe>().align();
        let ptr = unsafe {
            let probe_layout = Layout::from_size_align_unchecked(Probe::SIZE, probe_align);
            alloc_zeroed(probe_layout)
        };

        ptr::NonNull::new(ptr)
            .map(ptr::NonNull::cast)
            .map(Probe)
            .expect("Probe alloc failed!")
    }

    #[inline]
    pub(crate) fn as_mut_ptr(&mut self) -> *mut sys::io_uring_probe {
        self.0.as_ptr()
    }

    pub fn is_supported(&self, opcode: u8) -> bool {
        unsafe {
            let probe = &*self.0.as_ptr();

            if opcode <= probe.last_op {
                let ops = probe.ops.as_slice(Self::COUNT);
                ops[opcode as usize].flags & (sys::IO_URING_OP_SUPPORTED as u16) != 0
            } else {
                false
            }
        }
    }
}

impl Default for Probe {
    #[inline]
    fn default() -> Probe {
        Probe::new()
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        use std::alloc::{ dealloc, Layout };

        let probe_align = Layout::new::<sys::io_uring_probe>().align();
        unsafe {
            let probe_layout = Layout::from_size_align_unchecked(Probe::SIZE, probe_align);
            dealloc(self.0.as_ptr() as *mut _, probe_layout);
        }
    }
}
