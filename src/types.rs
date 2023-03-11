//! Common Linux types not provided by libc.

pub(crate) mod sealed {
    use super::{Fd, Fixed};
    use std::os::unix::io::RawFd;

    #[derive(Debug)]
    pub enum Target {
        Fd(RawFd),
        Fixed(u32),
    }

    pub trait UseFd: Sized {
        fn into(self) -> RawFd;
    }

    pub trait UseFixed: Sized {
        fn into(self) -> Target;
    }

    impl UseFd for Fd {
        #[inline]
        fn into(self) -> RawFd {
            self.0
        }
    }

    impl UseFixed for Fd {
        #[inline]
        fn into(self) -> Target {
            Target::Fd(self.0)
        }
    }

    impl UseFixed for Fixed {
        #[inline]
        fn into(self) -> Target {
            Target::Fixed(self.0)
        }
    }
}

use crate::sys;
use bitflags::bitflags;
use std::num::NonZeroU32;
use std::os::unix::io::RawFd;

use std::marker::PhantomData;

use crate::util::cast_ptr;

pub use sys::__kernel_rwf_t as RwFlags;

/// Opaque types, you should use [`statx`](struct@libc::statx) instead.
#[repr(C)]
pub struct statx {
    _priv: (),
}

/// Opaque types, you should use [`epoll_event`](libc::epoll_event) instead.
#[repr(C)]
pub struct epoll_event {
    _priv: (),
}

/// A file descriptor that has not been registered with io_uring.
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct Fd(pub RawFd);

/// A file descriptor that has been registered with io_uring using
/// [`Submitter::register_files`](crate::Submitter::register_files) or [`Submitter::register_files_sparse`](crate::Submitter::register_files_sparse).
/// This can reduce overhead compared to using [`Fd`] in some cases.
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct Fixed(pub u32);

bitflags! {
    /// Options for [`Timeout`](super::Timeout).
    pub struct TimeoutFlags: u32 {
        const ABS = sys::IORING_TIMEOUT_ABS;

        const UPDATE = sys::IORING_TIMEOUT_UPDATE;
    }
}

bitflags! {
    /// Options for [`Fsync`](super::Fsync).
    pub struct FsyncFlags: u32 {
        const DATASYNC = sys::IORING_FSYNC_DATASYNC;
    }
}

/// Wrapper around `open_how` as used in [the `openat2(2)` system
/// call](https://man7.org/linux/man-pages/man2/openat2.2.html).
#[derive(Default, Debug, Clone, Copy)]
#[repr(transparent)]
pub struct OpenHow(sys::open_how);

impl OpenHow {
    pub const fn new() -> Self {
        OpenHow(sys::open_how {
            flags: 0,
            mode: 0,
            resolve: 0,
        })
    }

    pub const fn flags(mut self, flags: u64) -> Self {
        self.0.flags = flags;
        self
    }

    pub const fn mode(mut self, mode: u64) -> Self {
        self.0.mode = mode;
        self
    }

    pub const fn resolve(mut self, resolve: u64) -> Self {
        self.0.resolve = resolve;
        self
    }
}

#[derive(Default, Debug, Clone, Copy)]
#[repr(transparent)]
pub struct Timespec(sys::__kernel_timespec);

impl Timespec {
    #[inline]
    pub const fn new() -> Self {
        Timespec(sys::__kernel_timespec {
            tv_sec: 0,
            tv_nsec: 0,
        })
    }

    #[inline]
    pub const fn sec(mut self, sec: u64) -> Self {
        self.0.tv_sec = sec as _;
        self
    }

    #[inline]
    pub const fn nsec(mut self, nsec: u32) -> Self {
        self.0.tv_nsec = nsec as _;
        self
    }
}

/// Submit arguments
///
/// Note that arguments that exceed their lifetime will fail to compile.
///
/// ```compile_fail
/// use io_uring::types::{ SubmitArgs, Timespec };
///
/// let sigmask: libc::sigset_t = unsafe { std::mem::zeroed() };
///
/// let mut args = SubmitArgs::new();
///
/// {
///     let ts = Timespec::new();
///     args = args.timespec(&ts);
///     args = args.sigmask(&sigmask);
/// }
///
/// drop(args);
/// ```
#[derive(Default, Debug, Clone, Copy)]
pub struct SubmitArgs<'prev: 'now, 'now> {
    pub(crate) args: sys::io_uring_getevents_arg,
    prev: PhantomData<&'prev ()>,
    now: PhantomData<&'now ()>,
}

impl<'prev, 'now> SubmitArgs<'prev, 'now> {
    #[inline]
    pub const fn new() -> SubmitArgs<'static, 'static> {
        let args = sys::io_uring_getevents_arg {
            sigmask: 0,
            sigmask_sz: 0,
            pad: 0,
            ts: 0,
        };

        SubmitArgs {
            args,
            prev: PhantomData,
            now: PhantomData,
        }
    }

    #[inline]
    pub fn sigmask<'new>(mut self, sigmask: &'new libc::sigset_t) -> SubmitArgs<'now, 'new> {
        self.args.sigmask = cast_ptr(sigmask) as _;
        self.args.sigmask_sz = std::mem::size_of::<libc::sigset_t>() as _;

        SubmitArgs {
            args: self.args,
            prev: self.now,
            now: PhantomData,
        }
    }

    #[inline]
    pub fn timespec<'new>(mut self, timespec: &'new Timespec) -> SubmitArgs<'now, 'new> {
        self.args.ts = cast_ptr(timespec) as _;

        SubmitArgs {
            args: self.args,
            prev: self.now,
            now: PhantomData,
        }
    }
}

#[repr(transparent)]
pub struct BufRingEntry(sys::io_uring_buf);

/// An entry in a buf_ring that allows setting the address, length and buffer id.
impl BufRingEntry {
    /// Sets the entry addr.
    pub fn set_addr(&mut self, addr: u64) {
        self.0.addr = addr;
    }
    /// Returns the entry addr.
    pub fn addr(&self) -> u64 {
        self.0.addr
    }
    /// Sets the entry len.
    pub fn set_len(&mut self, len: u32) {
        self.0.len = len;
    }
    /// Returns the entry len.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> u32 {
        self.0.len
    }
    /// Sets the entry bid.
    pub fn set_bid(&mut self, bid: u16) {
        self.0.bid = bid;
    }
    /// Returns the entry bid.
    pub fn bid(&self) -> u16 {
        self.0.bid
    }

    /// The offset to the ring's tail field given the ring's base address.
    ///
    /// The caller should ensure the ring's base address is aligned with the system's page size,
    /// per the uring interface requirements.
    ///
    /// # Safety
    ///
    /// The ptr will be dereferenced in order to determine the address of the resv field,
    /// so the caller is responsible for passing in a valid pointer. And not just
    /// a valid pointer type, but also the argument must be the address to the first entry
    /// of the buf_ring for the resv field to even be considered the tail field of the ring.
    /// The entry must also be properly initialized.
    pub unsafe fn tail(ring_base: *const BufRingEntry) -> *const u16 {
        &(*ring_base).0.resv
    }
}

/// Convert a valid `u32` constant.
///
/// This is a workaround for the lack of panic-in-const in older
/// toolchains.
#[allow(unconditional_panic)]
const fn unwrap_u32(t: Option<u32>) -> u32 {
    match t {
        Some(v) => v,
        None => [][1],
    }
}

/// Convert a valid `NonZeroU32` constant.
///
/// This is a workaround for the lack of panic-in-const in older
/// toolchains.
#[allow(unconditional_panic)]
const fn unwrap_nonzero(t: Option<NonZeroU32>) -> NonZeroU32 {
    match t {
        Some(v) => v,
        None => [][1],
    }
}

/// A destination slot for sending fixed resources
/// (e.g. [`opcode::MsgRingSendFd`](crate::opcode::MsgRingSendFd)).
#[derive(Debug, Clone, Copy)]
pub struct DestinationSlot {
    /// Fixed slot as indexed by the kernel (target+1).
    dest: NonZeroU32,
}

impl DestinationSlot {
    // SAFETY: kernel constant, `IORING_FILE_INDEX_ALLOC` is always > 0.
    const AUTO_ALLOC: NonZeroU32 =
        unwrap_nonzero(NonZeroU32::new(sys::IORING_FILE_INDEX_ALLOC as u32));

    /// Use an automatically allocated target slot.
    pub const fn auto_target() -> Self {
        Self {
            dest: DestinationSlot::AUTO_ALLOC,
        }
    }

    /// Try to use a given target slot.
    ///
    /// Valid slots are in the range from `0` to `u32::MAX - 2` inclusive.
    pub fn try_from_slot_target(target: u32) -> Result<Self, u32> {
        // SAFETY: kernel constant, `IORING_FILE_INDEX_ALLOC` is always >= 2.
        const MAX_INDEX: u32 = unwrap_u32(DestinationSlot::AUTO_ALLOC.get().checked_sub(2));

        if target > MAX_INDEX {
            return Err(target);
        }

        let kernel_index = target.saturating_add(1);
        // SAFETY: by construction, always clamped between 1 and IORING_FILE_INDEX_ALLOC-1.
        debug_assert!(0 < kernel_index && kernel_index < DestinationSlot::AUTO_ALLOC.get());
        let dest = NonZeroU32::new(kernel_index).unwrap();

        Ok(Self { dest })
    }

    pub(crate) fn kernel_index_arg(&self) -> u32 {
        self.dest.get()
    }
}

/// Helper structure for parsing the result of a multishot [`opcode::RecvMsg`](crate::opcode::RecvMsg).
#[derive(Debug)]
pub struct RecvMsgOut<'buf> {
    header: sys::io_uring_recvmsg_out,
    /// The fixed length of the name field, in bytes.
    ///
    /// If the incoming name data is larger than this, it gets truncated to this.
    /// If it is smaller, it gets 0-padded to fill the whole field. In either case,
    /// this fixed amount of space is reserved in the result buffer.
    msghdr_name_len: usize,
    /// The fixed length of the control field, in bytes.
    ///
    /// This follows the same semantics as the field above, but for control data.
    msghdr_control_len: usize,
    name_data: &'buf [u8],
    control_data: &'buf [u8],
    payload_data: &'buf [u8],
}

impl<'buf> RecvMsgOut<'buf> {
    const DATA_START: usize = std::mem::size_of::<sys::io_uring_recvmsg_out>();

    /// Parse the data buffered upon completion of a `RecvMsg` multishot operation.
    ///
    /// `buffer` is the whole buffer previously provided to the ring, while `msghdr`
    /// is the same content provided as input to the corresponding SQE
    /// (only `msg_namelen` and `msg_controllen` fields are relevant).
    pub fn parse(buffer: &'buf [u8], msghdr: &libc::msghdr) -> Result<Self, ()> {
        if buffer.len() < std::mem::size_of::<sys::io_uring_recvmsg_out>() {
            return Err(());
        }
        // SAFETY: buffer (minimum) length is checked here above.
        let header: sys::io_uring_recvmsg_out =
            unsafe { std::ptr::read_unaligned(buffer.as_ptr() as _) };

        let msghdr_name_len = msghdr.msg_namelen as _;
        let msghdr_control_len = msghdr.msg_controllen as _;

        // Check total length upfront, so that further logic here
        // below can safely use unchecked/saturating math.
        let length_overflow = Some(Self::DATA_START)
            .and_then(|acc| acc.checked_add(msghdr_name_len))
            .and_then(|acc| acc.checked_add(msghdr_control_len))
            .and_then(|acc| acc.checked_add(header.payloadlen as usize))
            .map(|total_len| total_len > buffer.len())
            .unwrap_or(true);
        if length_overflow {
            return Err(());
        }

        let (name_data, control_start) = {
            let name_start = Self::DATA_START;
            let name_size = usize::min(header.namelen as usize, msghdr_name_len);
            let name_data_end = name_start.saturating_add(name_size);
            let name_data = &buffer[name_start..name_data_end];
            let name_field_end = name_start.saturating_add(msghdr_name_len);
            (name_data, name_field_end)
        };
        let (control_data, payload_start) = {
            let control_size = usize::min(header.controllen as usize, msghdr_control_len);
            let control_data_end = control_start.saturating_add(control_size);
            let control_data = &buffer[control_start..control_data_end];
            let control_field_end = control_start.saturating_add(msghdr_control_len);
            (control_data, control_field_end)
        };
        let payload_data = {
            let payload_data_end = payload_start.saturating_add(header.payloadlen as usize);
            &buffer[payload_start..payload_data_end]
        };

        Ok(Self {
            header,
            msghdr_name_len,
            msghdr_control_len,
            name_data,
            control_data,
            payload_data,
        })
    }

    /// Return the length of the incoming `name` data.
    ///
    /// This may be larger than the size of the content returned by
    /// `name_data()`, if the kernel could not fit all the incoming
    /// data in the provided buffer size. In that case, name data in
    /// the result buffer gets truncated.
    pub fn incoming_name_len(&self) -> u32 {
        self.header.namelen
    }

    /// Return whether the incoming name data was larger than the provided limit/buffer.
    ///
    /// When `true`, data returned by `name_data()` is truncated and
    /// incomplete.
    pub fn is_name_data_truncated(&self) -> bool {
        self.header.namelen as usize > self.msghdr_name_len
    }

    /// Message control data, with the same semantics as `msghdr.msg_control`.
    pub fn name_data(&self) -> &[u8] {
        self.name_data
    }

    /// Return the length of the incoming `control` data.
    ///
    /// This may be larger than the size of the content returned by
    /// `control_data()`, if the kernel could not fit all the incoming
    /// data in the provided buffer size. In that case, control data in
    /// the result buffer gets truncated.
    pub fn incoming_control_len(&self) -> u32 {
        self.header.controllen
    }

    /// Return whether the incoming control data was larger than the provided limit/buffer.
    ///
    /// When `true`, data returned by `control_data()` is truncated and
    /// incomplete.
    pub fn is_control_data_truncated(&self) -> bool {
        self.header.controllen as usize > self.msghdr_control_len
    }

    /// Message control data, with the same semantics as `msghdr.msg_control`.
    pub fn control_data(&self) -> &[u8] {
        self.control_data
    }

    /// Return whether the incoming payload was larger than the provided limit/buffer.
    ///
    /// When `true`, data returned by `payload_data()` is truncated and
    /// incomplete.
    pub fn is_payload_truncated(&self) -> bool {
        self.header.flags & (libc::MSG_TRUNC as u32) != 0
    }

    /// Message payload, as buffered by the kernel.
    pub fn payload_data(&self) -> &[u8] {
        self.payload_data
    }

    /// Message flags, with the same semantics as `msghdr.msg_flags`.
    pub fn flags(&self) -> u32 {
        self.header.flags
    }
}
