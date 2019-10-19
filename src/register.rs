use std::io::IoSlice;
use std::os::unix::io::RawFd;
use linux_io_uring_sys as sys;


#[allow(clippy::module_inception)]
pub mod register {
    use super::*;

    pub enum Target<'a, 'b> {
        Buffer(&'b [IoSlice<'a>]),
        File(&'b [RawFd]),
        Event(&'b [RawFd]),

        // TODO https://github.com/rust-lang/rust/pull/64639
        #[doc(hidden)]
        __Unknown
    }

    impl Target<'_, '_> {
        pub(crate) fn export(&self) -> (libc::c_uint, *const libc::c_void, libc::c_uint) {
            match self {
                Target::Buffer(bufs) =>
                    (sys::IORING_REGISTER_BUFFERS, bufs.as_ptr() as *const _, bufs.len() as _),
                Target::File(fds) =>
                    (sys::IORING_REGISTER_FILES, fds.as_ptr() as *const _, fds.len() as _),
                Target::Event(events)
                    => (sys::IORING_REGISTER_EVENTFD, events.as_ptr() as *const _, events.len() as _),
                _ => unreachable!()
            }
        }
    }
}

pub mod unregister {
    use super::*;

    pub enum Target {
        Buffer,
        File,
        Event,

        #[doc(hidden)]
        __Unknown
    }

    impl Target {
        pub(crate) fn opcode(&self) -> libc::c_uint {
            match self {
                Target::Buffer => sys::IORING_UNREGISTER_BUFFERS,
                Target::File => sys::IORING_UNREGISTER_FILES,
                Target::Event => sys::IORING_UNREGISTER_EVENTFD,
                _ => unreachable!()
            }
        }
    }
}
