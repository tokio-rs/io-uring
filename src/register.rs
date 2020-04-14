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

/// Register files or user buffers for asynchronous I/O.
#[allow(clippy::module_inception)]
pub mod register {
    use super::*;

    #[non_exhaustive]
    pub enum Target<'a> {
        /// Register buffer,
        /// then use the already registered buffer in
        /// [ReadFixed](crate::opcode::ReadFixed) and [WriteFixed](crate::opcode::WriteFixed).
        Buffers(&'a [libc::iovec]),

        /// Register file descriptor,
        /// then use the already registered fd in [Target::Fixed](crate::opcode::types::Target).
        Files(&'a [RawFd]),

        /// Register eventfd.
        EventFd(RawFd),

        FilesUpdate { offset: u32, fds: &'a [RawFd] },
    }

    impl Target<'_> {
        pub(crate) fn execute(&self, fd: RawFd) -> io::Result<()> {
            fn cast_ptr<T>(n: &T) -> *const T {
                n as *const T
            }

            match self {
                Target::Buffers(bufs) =>
                    execute(fd, sys::IORING_REGISTER_BUFFERS, bufs.as_ptr() as *const _, bufs.len() as _)?,
                Target::Files(fds) =>
                    execute(fd, sys::IORING_REGISTER_FILES, fds.as_ptr() as *const _, fds.len() as _)?,
                Target::EventFd(eventfd) =>
                    execute(fd, sys::IORING_REGISTER_EVENTFD, cast_ptr::<RawFd>(eventfd) as *const _, 1)?,
                Target::FilesUpdate { offset, fds } => {
                    let fu = sys::io_uring_files_update {
                        offset: *offset,
                        resv: 0,
                        fds: fds.as_ptr() as _
                    };
                    let fu = &fu as *const sys::io_uring_files_update;

                    execute(fd, sys::IORING_REGISTER_FILES_UPDATE, fu as *const _, fds.len() as _)?
                },
            };

            Ok(())
        }
    }
}

/// Unregister files or user buffers for asynchronous I/O.
pub mod unregister {
    use super::*;

    #[non_exhaustive]
    pub enum Target {
        /// Unregister buffer.
        Buffers,

        /// Unregister file descriptor.
        Files,

        /// Unregister eventfd.
        EventFd,
    }

    impl Target {
        pub(crate) fn execute(&self, fd: RawFd) -> io::Result<()> {
            let opcode = match self {
                Target::Buffers => sys::IORING_UNREGISTER_BUFFERS,
                Target::Files => sys::IORING_UNREGISTER_FILES,
                Target::EventFd => sys::IORING_UNREGISTER_EVENTFD,
            };

            execute(fd, opcode, ptr::null(), 0)?;

            Ok(())
        }
    }
}

pub struct Probe(ptr::NonNull<sys::io_uring_probe>);

impl Probe {
    const COUNT: usize = 256;
    const SIZE: usize = mem::size_of::<sys::io_uring_probe>()
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
