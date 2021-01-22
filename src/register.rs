use std::os::unix::io::RawFd;
use std::{io, mem, ptr};

use crate::sys;

pub(crate) fn execute(
    fd: RawFd,
    opcode: libc::c_uint,
    arg: *const libc::c_void,
    len: libc::c_uint,
) -> io::Result<i32> {
    unsafe {
        let ret = sys::io_uring_register(fd, opcode, arg, len);
        if ret >= 0 {
            Ok(ret)
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

/// Information about what `io_uring` features the kernel supports.
///
/// You can fill this in with [`register_probe`](crate::Submitter::register_probe).
pub struct Probe(ptr::NonNull<sys::io_uring_probe>);

impl Probe {
    pub(crate) const COUNT: usize = 256;
    pub(crate) const SIZE: usize = mem::size_of::<sys::io_uring_probe>()
        + Self::COUNT * mem::size_of::<sys::io_uring_probe_op>();

    /// Create a new probe with no features enabled.
    #[allow(clippy::cast_ptr_alignment)]
    pub fn new() -> Probe {
        use std::alloc::{alloc_zeroed, Layout};

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

    /// Get whether a specific opcode is supported.
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
        use std::alloc::{dealloc, Layout};

        let probe_align = Layout::new::<sys::io_uring_probe>().align();
        unsafe {
            let probe_layout = Layout::from_size_align_unchecked(Probe::SIZE, probe_align);
            dealloc(self.0.as_ptr() as *mut _, probe_layout);
        }
    }
}

/// An id associated with the credentials of the current process. After [registering](crate::Submitter::register_personality)
/// the id, it can be [passed](crate::squeue::Entry::personality) into submission queue entries
/// to issue the request with the process' credentials.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Personality {
    pub(crate) id: u16,
}

/// An allowed feature of io_uring. You can set the allowed features with
/// [`register_restrictions`](crate::Submitter::register_restrictions).
///
/// Requires the `unstable` feature.
#[cfg(feature = "unstable")]
#[repr(transparent)]
pub struct Restriction(sys::io_uring_restriction);

/// inline zeroed to improve codegen
#[cfg(feature = "unstable")]
#[inline(always)]
fn res_zeroed() -> sys::io_uring_restriction {
    unsafe { std::mem::zeroed() }
}

#[cfg(feature = "unstable")]
impl Restriction {
    /// Allow an `io_uring_register` opcode.
    pub fn register_op(op: u8) -> Restriction {
        let mut res = res_zeroed();
        res.opcode = sys::IORING_RESTRICTION_REGISTER_OP as _;
        res.__bindgen_anon_1.register_op = op;
        Restriction(res)
    }

    /// Allow a submission queue event opcode.
    pub fn sqe_op(op: u8) -> Restriction {
        let mut res = res_zeroed();
        res.opcode = sys::IORING_RESTRICTION_SQE_OP as _;
        res.__bindgen_anon_1.sqe_op = op;
        Restriction(res)
    }

    /// Allow the given [submission queue event flags](crate::squeue::Flags).
    pub fn sqe_flags_allowed(flags: u8) -> Restriction {
        let mut res = res_zeroed();
        res.opcode = sys::IORING_RESTRICTION_SQE_FLAGS_ALLOWED as _;
        res.__bindgen_anon_1.sqe_flags = flags;
        Restriction(res)
    }

    /// Require the given [submission queue event flags](crate::squeue::Flags). These flags must be
    /// set on every submission.
    pub fn sqe_flags_required(flags: u8) -> Restriction {
        let mut res = res_zeroed();
        res.opcode = sys::IORING_RESTRICTION_SQE_FLAGS_REQUIRED as _;
        res.__bindgen_anon_1.sqe_flags = flags;
        Restriction(res)
    }
}
