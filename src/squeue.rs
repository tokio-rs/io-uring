//! Submission Queue

use std::sync::atomic;

use crate::sys;
use crate::util::{unsync_load, Mmap};

use bitflags::bitflags;

/// An io_uring instance's submission queue. This contains all the I/O operations the application
/// sends to the kernel.
pub struct SubmissionQueue {
    pub(crate) head: *const atomic::AtomicU32,
    pub(crate) tail: *const atomic::AtomicU32,
    pub(crate) ring_mask: *const u32,
    pub(crate) ring_entries: *const u32,
    pub(crate) flags: *const atomic::AtomicU32,
    dropped: *const atomic::AtomicU32,

    pub(crate) sqes: *mut sys::io_uring_sqe,
}

/// A snapshot of the submission queue.
pub struct AvailableQueue<'a> {
    head: u32,
    tail: u32,
    ring_mask: u32,
    ring_entries: u32,
    queue: &'a mut SubmissionQueue,
}

/// An entry in the submission queue, representing a request for an I/O operation.
///
/// These can be created via the opcodes in [`opcode`](crate::opcode).
#[repr(transparent)]
#[derive(Clone)]
pub struct Entry(pub(crate) sys::io_uring_sqe);

bitflags! {
    /// Submission flags
    pub struct Flags: u8 {
        /// When this flag is specified,
        /// `fd` is an index into the files array registered with the io_uring instance.
        #[doc(hidden)]
        const FIXED_FILE = 1 << sys::IOSQE_FIXED_FILE_BIT;

        /// When this flag is specified,
        /// the SQE will not be started before previously submitted SQEs have completed,
        /// and new SQEs will not be started before this one completes.
        const IO_DRAIN = 1 << sys::IOSQE_IO_DRAIN_BIT;

        /// When this flag is specified,
        /// it forms a link with the next SQE in the submission ring.
        /// That next SQE will not be started before this one completes.
        const IO_LINK = 1 << sys::IOSQE_IO_LINK_BIT;

        /// Like [`IO_LINK`](Self::IO_LINK), but it doesn’t sever regardless of the completion
        /// result.
        const IO_HARDLINK = 1 << sys::IOSQE_IO_HARDLINK_BIT;

        /// Normal operation for io_uring is to try and issue an sqe as non-blocking first,
        /// and if that fails, execute it in an async manner.
        ///
        /// To support more efficient overlapped operation of requests
        /// that the application knows/assumes will always (or most of the time) block,
        /// the application can ask for an sqe to be issued async from the start.
        const ASYNC = 1 << sys::IOSQE_ASYNC_BIT;

        /// Conceptually the kernel holds a set of buffers organized into groups. When you issue a
        /// request with this flag and set `buf_group` to a valid buffer group ID (e.g.
        /// [`buf_group` on `Read`](crate::opcode::Read::buf_group)) then once the file descriptor
        /// becomes ready the kernel will try to take a buffer from the group.
        ///
        /// If there are no buffers in the group, your request will fail with `-ENOBUFS`. Otherwise,
        /// the corresponding [`cqueue::Entry::flags`](crate::cqueue::Entry::flags) will contain the
        /// chosen buffer ID, encoded with:
        ///
        /// ```text
        /// (buffer_id << IORING_CQE_BUFFER_SHIFT) | IORING_CQE_F_BUFFER
        /// ```
        ///
        /// You can use [`buffer_select`](crate::cqueue::buffer_select) to take the buffer ID.
        ///
        /// The buffer will then be removed from the group and won't be usable by other requests
        /// anymore.
        ///
        /// You can provide new buffers in a group with
        /// [`ProvideBuffers`](crate::opcode::ProvideBuffers).
        ///
        /// See also [the LWN thread on automatic buffer
        /// selection](https://lwn.net/Articles/815491/).
        ///
        /// Requires the `unstable` feature.
        #[cfg(feature = "unstable")]
        const BUFFER_SELECT = 1 << sys::IOSQE_BUFFER_SELECT_BIT;
    }
}

impl SubmissionQueue {
    pub(crate) unsafe fn new(
        sq_mmap: &Mmap,
        sqe_mmap: &Mmap,
        p: &sys::io_uring_params,
    ) -> SubmissionQueue {
        mmap_offset! {
            let head            = sq_mmap + p.sq_off.head           => *const atomic::AtomicU32;
            let tail            = sq_mmap + p.sq_off.tail           => *const atomic::AtomicU32;
            let ring_mask       = sq_mmap + p.sq_off.ring_mask      => *const u32;
            let ring_entries    = sq_mmap + p.sq_off.ring_entries   => *const u32;
            let flags           = sq_mmap + p.sq_off.flags          => *const atomic::AtomicU32;
            let dropped         = sq_mmap + p.sq_off.dropped        => *const atomic::AtomicU32;
            let array           = sq_mmap + p.sq_off.array          => *mut u32;

            let sqes            = sqe_mmap + 0                      => *mut sys::io_uring_sqe;
        }

        // To keep it simple, map it directly to `sqes`.
        for i in 0..*ring_entries {
            array.add(i as usize).write_volatile(i);
        }

        SubmissionQueue {
            head,
            tail,
            ring_mask,
            ring_entries,
            flags,
            dropped,
            sqes,
        }
    }

    /// When [`is_setup_sqpoll`](crate::Parameters::is_setup_sqpoll) is set, whether the kernel
    /// threads has gone to sleep and requires a system call to wake it up.
    pub fn need_wakeup(&self) -> bool {
        unsafe { (*self.flags).load(atomic::Ordering::Acquire) & sys::IORING_SQ_NEED_WAKEUP != 0 }
    }

    /// The number of invalid submission queue entries that have been encountered in the ring
    /// buffer.
    pub fn dropped(&self) -> u32 {
        unsafe { (*self.dropped).load(atomic::Ordering::Acquire) }
    }

    /// Returns `true` if the completion queue ring is overflown.
    ///
    /// Requires the `unstable` feature.
    #[cfg(feature = "unstable")]
    pub fn cq_overflow(&self) -> bool {
        unsafe { (*self.flags).load(atomic::Ordering::Acquire) & sys::IORING_SQ_CQ_OVERFLOW != 0 }
    }

    /// Get the total number of entries in the submission queue ring buffer.
    #[inline]
    pub fn capacity(&self) -> usize {
        unsafe { self.ring_entries.read() as usize }
    }

    /// Get the number of submission queue events in the ring buffer.
    #[inline]
    pub fn len(&self) -> usize {
        unsafe {
            let head = (*self.head).load(atomic::Ordering::Acquire);
            let tail = unsync_load(self.tail);

            tail.wrapping_sub(head) as usize
        }
    }

    /// Returns `true` if the submission queue ring buffer is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns `true` if the submission queue ring buffer has reached capacity, and no more events
    /// can be added before the kernel consumes some.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }

    /// Take a snapshot of the submission queue.
    pub fn available(&mut self) -> AvailableQueue<'_> {
        unsafe {
            AvailableQueue {
                head: (*self.head).load(atomic::Ordering::Acquire),
                tail: unsync_load(self.tail),
                ring_mask: self.ring_mask.read(),
                ring_entries: self.ring_entries.read(),
                queue: self,
            }
        }
    }
}

impl AvailableQueue<'_> {
    /// Synchronize this snapshot with the real queue.
    pub fn sync(&mut self) {
        unsafe {
            (*self.queue.tail).store(self.tail, atomic::Ordering::Release);
            self.head = (*self.queue.head).load(atomic::Ordering::Acquire);
        }
    }

    /// Get the total number of entries in the submission queue ring buffer.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.ring_entries as usize
    }

    /// Get the number of submission queue events in the ring buffer.
    #[inline]
    pub fn len(&self) -> usize {
        self.tail.wrapping_sub(self.head) as usize
    }

    /// Returns `true` if the submission queue ring buffer is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    /// Returns `true` if the submission queue ring buffer has reached capacity, and no more events
    /// can be added before the kernel consumes some.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.tail.wrapping_sub(self.head) == self.ring_entries
    }

    /// Attempts to push an [`Entry`] into the queue.
    /// If the queue is full, the element is returned back as an error.
    ///
    /// # Safety
    ///
    /// Developers must ensure that parameters of the [`Entry`] (such as buffer) are valid and will
    /// be valid for the entire duration of the operation, otherwise it may cause memory problems.
    pub unsafe fn push(&mut self, Entry(entry): Entry) -> Result<(), Entry> {
        if !self.is_full() {
            *self.queue.sqes.add((self.tail & self.ring_mask) as usize) = entry;
            self.tail = self.tail.wrapping_add(1);
            Ok(())
        } else {
            Err(Entry(entry))
        }
    }
}

impl Drop for AvailableQueue<'_> {
    fn drop(&mut self) {
        unsafe {
            (*self.queue.tail).store(self.tail, atomic::Ordering::Release);
        }
    }
}

impl Entry {
    /// Set the submission event's [flags](Flags).
    pub fn flags(mut self, flags: Flags) -> Entry {
        self.0.flags |= flags.bits();
        self
    }

    /// Set the user data. This is an application-supplied value that will be passed straight
    /// through into the [completion queue entry](crate::cqueue::Entry::user_data).
    pub fn user_data(mut self, user_data: u64) -> Entry {
        self.0.user_data = user_data;
        self
    }
}
