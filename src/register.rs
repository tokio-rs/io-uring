use linux_io_uring_sys as sys;
use std::os::unix::io::RawFd;

#[allow(clippy::module_inception)]
pub mod register {
    use super::*;

    pub enum Target<'a> {
        Buffer(&'a [libc::iovec]),
        File(&'a [RawFd]),
        Event(RawFd),

        // TODO https://github.com/rust-lang/rust/pull/64639
        #[doc(hidden)]
        __Unknown,
    }

    impl Target<'_> {
        pub(crate) fn export(&self) -> (libc::c_uint, *const libc::c_void, libc::c_uint) {
            fn cast_ptr<T>(n: &T) -> *const T {
                n as *const T
            }

            match self {
                Target::Buffer(bufs) => (
                    sys::IORING_REGISTER_BUFFERS,
                    bufs.as_ptr() as *const _,
                    bufs.len() as _,
                ),
                Target::File(fds) => (
                    sys::IORING_REGISTER_FILES,
                    fds.as_ptr() as *const _,
                    fds.len() as _,
                ),
                Target::Event(eventfd) => (
                    sys::IORING_REGISTER_EVENTFD,
                    cast_ptr::<RawFd>(eventfd) as *const _,
                    1,
                ),
                _ => unreachable!(),
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
        __Unknown,
    }

    impl Target {
        pub(crate) fn opcode(&self) -> libc::c_uint {
            match self {
                Target::Buffer => sys::IORING_UNREGISTER_BUFFERS,
                Target::File => sys::IORING_UNREGISTER_FILES,
                Target::Event => sys::IORING_UNREGISTER_EVENTFD,
                _ => unreachable!(),
            }
        }
    }
}
