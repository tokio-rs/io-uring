use std::{ io, ptr };
use std::os::unix::io::RawFd;
use crate::sys;


fn execute(fd: RawFd, opcode: libc::c_uint, arg: *const libc::c_void, len: libc::c_uint)
    -> io::Result<()>
{
    unsafe {
        let ret = sys::io_uring_register(fd, opcode, arg, len);
        if ret >= 0 {
            Ok(())
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

        /// 5.6 available
        #[cfg(feature = "unstable")]
        EventFdAsync(RawFd),

        /// 5.6 available
        #[cfg(feature = "unstable")]
        Probe(&'a mut Probe),

        /// 5.6 available
        #[cfg(feature = "unstable")]
        Personality
    }

    impl Target<'_> {
        pub(crate) fn execute(&self, fd: RawFd) -> io::Result<()> {
            fn cast_ptr<T>(n: &T) -> *const T {
                n as *const T
            }

            match self {
                Target::Buffers(bufs) =>
                    execute(fd, sys::IORING_REGISTER_BUFFERS, bufs.as_ptr() as *const _, bufs.len() as _),
                Target::Files(fds) =>
                    execute(fd, sys::IORING_REGISTER_FILES, fds.as_ptr() as *const _, fds.len() as _),
                Target::EventFd(eventfd) =>
                    execute(fd, sys::IORING_REGISTER_EVENTFD, cast_ptr::<RawFd>(eventfd) as *const _, 1),
                Target::FilesUpdate { offset, fds } => {
                    let fu = sys::io_uring_files_update {
                        offset: *offset,
                        resv: 0,
                        fds: fds.as_ptr() as _
                    };
                    let fu = &fu as *const sys::io_uring_files_update;

                    execute(fd, sys::IORING_REGISTER_FILES_UPDATE, fu as *const _, fds.len() as _)
                },
                #[cfg(feature = "unstable")]
                Target::EventFdAsync(eventfd) =>
                    execute(fd, sys::IORING_REGISTER_EVENTFD_ASYNC, cast_ptr::<RawFd>(eventfd) as *const _, 1),
                #[cfg(feature = "unstable")]
                Target::Probe(probe) =>
                    execute(fd, sys::IORING_REGISTER_PROBE, probe.0.as_ptr() as *const _, 256),
                #[cfg(feature = "unstable")]
                Target::Personality =>
                    execute(fd, sys::IORING_REGISTER_PERSONALITY, ptr::null(), 0)
            }
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

        /// 5.6 available
        #[cfg(feature = "unstable")]
        Personality
    }

    impl Target {
        pub(crate) fn execute(&self, fd: RawFd) -> io::Result<()> {
            let opcode = match self {
                Target::Buffers => sys::IORING_UNREGISTER_BUFFERS,
                Target::Files => sys::IORING_UNREGISTER_FILES,
                Target::EventFd => sys::IORING_UNREGISTER_EVENTFD,
                #[cfg(feature = "unstable")]
                Target::Personality => sys::IORING_UNREGISTER_PERSONALITY
            };

            execute(fd, opcode, ptr::null(), 0)
        }
    }
}

#[cfg(feature = "unstable")]
use std::mem;

#[cfg(feature = "unstable")]
pub struct Probe(ptr::NonNull<sys::io_uring_probe>);

#[cfg(feature = "unstable")]
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

#[cfg(feature = "unstable")]
impl Default for Probe {
    #[inline]
    fn default() -> Probe {
        Probe::new()
    }
}

#[cfg(feature = "unstable")]
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
