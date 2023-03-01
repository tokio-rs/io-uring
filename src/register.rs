//! Some register syscall related types or parameters.

use core::{fmt, mem, ptr};

use crate::types::{
    IoringOp, IoringOpFlags, IoringRegisterOp, IoringRestrictionOp, IoringSqeFlags,
};

use crate::types::{AsFd, BorrowedFd};
use rustix::{io, io_uring};

pub(crate) fn execute<Fd: AsFd>(
    fd: Fd,
    opcode: IoringRegisterOp,
    arg: *const libc::c_void,
    len: u32,
) -> io::Result<u32> {
    unsafe { io_uring::io_uring_register(fd, opcode, arg, len) }
}

/// Information about what `io_uring` features the kernel supports.
///
/// You can fill this in with [`register_probe`](crate::Submitter::register_probe).
pub struct Probe(ptr::NonNull<io_uring::io_uring_probe>);

impl Probe {
    pub(crate) const COUNT: usize = 256;
    pub(crate) const SIZE: usize = mem::size_of::<io_uring::io_uring_probe>()
        + Self::COUNT * mem::size_of::<io_uring::io_uring_probe_op>();

    /// Create a new probe with no features enabled.
    #[allow(clippy::cast_ptr_alignment)]
    pub fn new() -> Probe {
        use alloc::alloc::alloc_zeroed;
        use core::alloc::Layout;

        let probe_align = Layout::new::<io_uring::io_uring_probe>().align();
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
    pub(crate) fn as_mut_ptr(&mut self) -> *mut io_uring::io_uring_probe {
        self.0.as_ptr()
    }

    /// Get whether a specific opcode is supported.
    pub fn is_supported(&self, opcode: IoringOp) -> bool {
        unsafe {
            let probe = &*self.0.as_ptr();

            if opcode as usize <= probe.last_op as usize {
                let ops = probe.ops.as_slice(Self::COUNT);
                ops[opcode as usize]
                    .flags
                    .intersects(IoringOpFlags::SUPPORTED)
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

impl fmt::Debug for Probe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        struct Op(io_uring::io_uring_probe_op);

        impl fmt::Debug for Op {
            #[inline]
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct("Op").field("code", &self.0.op).finish()
            }
        }

        let probe = unsafe { &*self.0.as_ptr() };
        let list = unsafe { probe.ops.as_slice(probe.last_op as usize + 1) };
        let list = list
            .iter()
            .filter(|op| op.flags.intersects(IoringOpFlags::SUPPORTED))
            .map(|&op| Op(op));

        f.debug_set().entries(list).finish()
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        use alloc::alloc::dealloc;
        use core::alloc::Layout;

        let probe_align = Layout::new::<io_uring::io_uring_probe>().align();
        unsafe {
            let probe_layout = Layout::from_size_align_unchecked(Probe::SIZE, probe_align);
            dealloc(self.0.as_ptr() as *mut _, probe_layout);
        }
    }
}

/// An allowed feature of io_uring. You can set the allowed features with
/// [`register_restrictions`](crate::Submitter::register_restrictions).
#[repr(transparent)]
pub struct Restriction(io_uring::io_uring_restriction);

/// inline zeroed to improve codegen
#[inline(always)]
fn res_zeroed() -> io_uring::io_uring_restriction {
    unsafe { core::mem::zeroed() }
}

impl Restriction {
    /// Allow an `io_uring_register` opcode.
    pub fn register_op(op: IoringRegisterOp) -> Restriction {
        let mut res = res_zeroed();
        res.opcode = IoringRestrictionOp::RegisterOp as _;
        res.register_or_sqe_op_or_sqe_flags.register_op = op;
        Restriction(res)
    }

    /// Allow a submission queue event opcode.
    pub fn sqe_op(op: IoringOp) -> Restriction {
        let mut res = res_zeroed();
        res.opcode = IoringRestrictionOp::SqeOp as _;
        res.register_or_sqe_op_or_sqe_flags.sqe_op = op;
        Restriction(res)
    }

    /// Allow the given [submission queue event flags](crate::squeue::Flags).
    pub fn sqe_flags_allowed(flags: IoringSqeFlags) -> Restriction {
        let mut res = res_zeroed();
        res.opcode = IoringRestrictionOp::SqeFlagsAllowed as _;
        res.register_or_sqe_op_or_sqe_flags.sqe_flags = flags;
        Restriction(res)
    }

    /// Require the given [submission queue event flags](crate::squeue::Flags). These flags must be
    /// set on every submission.
    pub fn sqe_flags_required(flags: IoringSqeFlags) -> Restriction {
        let mut res = res_zeroed();
        res.opcode = IoringRestrictionOp::SqeFlagsRequired as _;
        res.register_or_sqe_op_or_sqe_flags.sqe_flags = flags;
        Restriction(res)
    }
}

/// A RawFd, which can be used for
/// [register_files_update](crate::Submitter::register_files_update).
///
/// File descriptors can be skipped if they are set to `SKIP_FILE`.
/// Skipping an fd will not touch the file associated with the previous fd at that index.
pub const SKIP_FILE: BorrowedFd<'static> = io_uring::io_uring_register_files_skip();
