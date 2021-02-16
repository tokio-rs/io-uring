//! Completion Queue

#[cfg(feature = "unstable")]
use std::mem::MaybeUninit;
use std::ops::Deref;
use std::sync::atomic;

use crate::sys;
use crate::util::{unsync_load, Mmap};

/// An io_uring instance's completion queue. This stores all the I/O operations that have completed.
pub struct CompletionQueue {
    pub(crate) head: *const atomic::AtomicU32,
    pub(crate) tail: *const atomic::AtomicU32,
    pub(crate) ring_mask: *const u32,
    pub(crate) ring_entries: *const u32,

    overflow: *const atomic::AtomicU32,

    pub(crate) cqes: *const sys::io_uring_cqe,

    #[allow(dead_code)]
    flags: *const atomic::AtomicU32,
}

/// A type that grants mutable access to an io_uring's [completion queue](CompletionQueue).
///
/// This is necessary to prevent users swapping out the completion queue, which can cause
/// unsoundness.
pub struct Mut<'a>(pub(crate) &'a mut CompletionQueue);

/// An entry in the completion queue, representing a complete I/O operation.
#[repr(transparent)]
#[derive(Clone)]
pub struct Entry(pub(crate) sys::io_uring_cqe);

impl CompletionQueue {
    #[rustfmt::skip]
    pub(crate) unsafe fn new(cq_mmap: &Mmap, p: &sys::io_uring_params) -> CompletionQueue {
        let head         = cq_mmap.offset(p.cq_off.head         ) as *const atomic::AtomicU32;
        let tail         = cq_mmap.offset(p.cq_off.tail         ) as *const atomic::AtomicU32;
        let ring_mask    = cq_mmap.offset(p.cq_off.ring_mask    ) as *const u32;
        let ring_entries = cq_mmap.offset(p.cq_off.ring_entries ) as *const u32;
        let overflow     = cq_mmap.offset(p.cq_off.overflow     ) as *const atomic::AtomicU32;
        let cqes         = cq_mmap.offset(p.cq_off.cqes         ) as *const sys::io_uring_cqe;
        let flags        = cq_mmap.offset(p.cq_off.flags        ) as *const atomic::AtomicU32;

        CompletionQueue {
            head,
            tail,
            ring_mask,
            ring_entries,
            overflow,
            cqes,
            flags,
        }
    }

    /// If queue is full and [`is_feature_nodrop`](crate::Parameters::is_feature_nodrop) is not set,
    /// new events may be dropped. This records the number of dropped events.
    pub fn overflow(&self) -> u32 {
        unsafe { (*self.overflow).load(atomic::Ordering::Acquire) }
    }

    /// Whether eventfd notifications are disabled when a request is completed and queued to the CQ
    /// ring. This library currently does not provide a way to set it, so this will always be
    /// `false`.
    ///
    /// Requires the `unstable` feature.
    #[cfg(feature = "unstable")]
    pub fn eventfd_disabled(&self) -> bool {
        unsafe {
            (*self.flags).load(atomic::Ordering::Acquire) & sys::IORING_CQ_EVENTFD_DISABLED != 0
        }
    }

    /// Get the total number of entries in the completion queue ring buffer.
    #[inline]
    pub fn capacity(&self) -> usize {
        unsafe { self.ring_entries.read() as usize }
    }

    /// Get the number of unread completion queue events in the ring buffer.
    #[inline]
    pub fn len(&self) -> usize {
        unsafe {
            let head = unsync_load(self.head);
            let tail = (*self.tail).load(atomic::Ordering::Acquire);

            tail.wrapping_sub(head) as usize
        }
    }

    /// Returns `true` if there are no completion queue events to be processed.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns `true` if the completion queue is at maximum capacity. If
    /// [`is_feature_nodrop`](crate::Parameters::is_feature_nodrop) is not set, this will cause any
    /// new completion queue events to be dropped by the kernel.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }
}

impl Mut<'_> {
    /// Reborrow this mutable accessor to a shorter lifetime.
    ///
    /// This can be used to avoid consuming the `Mut` when passing it to functions.
    #[must_use]
    pub fn reborrow(&mut self) -> Mut<'_> {
        Mut(self.0)
    }

    #[cfg(feature = "unstable")]
    #[inline]
    pub fn fill(&mut self, entries: &mut [MaybeUninit<Entry>]) -> usize {
        let mut head = unsafe { unsync_load(self.0.head) };
        let tail = unsafe { &*self.0.tail }.load(atomic::Ordering::Acquire);

        let len = std::cmp::min(tail.wrapping_sub(head) as usize, entries.len()) as u32;

        for entry in &mut entries[..len as usize] {
            *entry = MaybeUninit::new(Entry(unsafe {
                *self.0.cqes.add((head & *self.ring_mask) as usize)
            }));
            head = head.wrapping_add(1);
        }

        unsafe { &*self.0.head }.store(head, atomic::Ordering::Release);

        len as usize
    }
}

impl Iterator for Mut<'_> {
    type Item = Entry;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let head = unsafe { unsync_load(self.0.head) };
        let tail = unsafe { &*self.0.tail }.load(atomic::Ordering::Acquire);

        if head != tail {
            let entry = unsafe { *self.0.cqes.add((head & *self.ring_mask) as usize) };
            unsafe { &*self.0.head }.fetch_add(1, atomic::Ordering::Release);
            Some(Entry(entry))
        } else {
            None
        }
    }
    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (
            unsafe {
                (*self.0.tail)
                    .load(atomic::Ordering::Acquire)
                    .wrapping_sub(unsync_load(self.0.head))
            } as usize,
            Some(self.capacity()),
        )
    }
}

impl Deref for Mut<'_> {
    type Target = CompletionQueue;
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl Entry {
    /// The operation-specific result code. For example, for a [`Read`](crate::opcode::Read)
    /// operation this is equivalent to the return value of the `read(2)` system call.
    #[inline]
    pub fn result(&self) -> i32 {
        self.0.res
    }

    /// The user data of the request, as set by
    /// [`Entry::user_data`](crate::squeue::Entry::user_data) on the submission queue event.
    #[inline]
    pub fn user_data(&self) -> u64 {
        self.0.user_data
    }

    /// Metadata related to the operation.
    ///
    /// This is currently used for:
    /// - Storing the selected buffer ID, if one was selected. See
    /// [`BUFFER_SELECT`](crate::squeue::Flags::BUFFER_SELECT) for more info.
    #[inline]
    pub fn flags(&self) -> u32 {
        self.0.flags
    }
}

#[cfg(feature = "unstable")]
pub fn buffer_select(flags: u32) -> Option<u16> {
    if flags & sys::IORING_CQE_F_BUFFER != 0 {
        let id = flags >> sys::IORING_CQE_BUFFER_SHIFT;

        // FIXME
        //
        // Should we return u16? maybe kernel will change value of `IORING_CQE_BUFFER_SHIFT` in future.
        Some(id as u16)
    } else {
        None
    }
}
