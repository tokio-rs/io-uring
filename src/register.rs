use std::{ io, ptr };
use std::os::unix::io::RawFd;
use linux_io_uring_sys as sys;


fn execute(fd: RawFd, opcode: libc::c_uint, arg: *const libc::c_void, len: libc::c_uint)
    -> io::Result<()>
{
    unsafe {
        if 0 == sys::io_uring_register(fd, opcode, arg, len) {
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
        Buffer(&'a [libc::iovec]),

        /// Register file descriptor,
        /// then use the already registered fd in [Target::Fixed](crate::opcode::Target).
        File(&'a [RawFd]),

        /// Register eventfd.
        Event(RawFd),

        FileUpdate { offset: u32, fds: &'a [RawFd] }
    }

    impl Target<'_> {
        pub(crate) fn execute(&self, fd: RawFd) -> io::Result<()> {
            fn cast_ptr<T>(n: &T) -> *const T {
                n as *const T
            }

            match self {
                Target::Buffer(bufs) =>
                    execute(fd, sys::IORING_REGISTER_BUFFERS, bufs.as_ptr() as *const _, bufs.len() as _),
                Target::File(fds) =>
                    execute(fd, sys::IORING_REGISTER_FILES, fds.as_ptr() as *const _, fds.len() as _),
                Target::Event(eventfd) =>
                    execute(fd, sys::IORING_REGISTER_EVENTFD, cast_ptr::<RawFd>(eventfd) as *const _, 1),
                Target::FileUpdate { offset, fds } => {
                    let fu = sys::io_uring_files_update {
                        offset: *offset,
                        fds: fds.as_ptr() as *mut _
                    };
                    let fu = &fu as *const sys::io_uring_files_update;

                    execute(fd, sys::IORING_REGISTER_FILES_UPDATE, fu as *const _, fds.len() as _)
                }
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
        Buffer,

        /// Unregister file descriptor.
        File,

        /// Unregister eventfd.
        Event,
    }

    impl Target {
        pub(crate) fn execute(&self, fd: RawFd) -> io::Result<()> {
            let opcode = match self {
                Target::Buffer => sys::IORING_UNREGISTER_BUFFERS,
                Target::File => sys::IORING_UNREGISTER_FILES,
                Target::Event => sys::IORING_UNREGISTER_EVENTFD
            };

            execute(fd, opcode, ptr::null(), 0)
        }
    }
}
