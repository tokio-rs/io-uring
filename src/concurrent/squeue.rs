use std::sync::atomic;

use crate::squeue::{self, Entry};

/// An io_uring instance's submission queue. This contains all the I/O operations the application
/// sends to the kernel.
pub struct SubmissionQueue<'a> {
    pub(crate) queue: &'a squeue::SubmissionQueue,
    pub(crate) reserved_tail: &'a atomic::AtomicU32,
    pub(crate) ring_mask: u32,
    pub(crate) ring_entries: u32,
}

impl SubmissionQueue<'_> {
    /// When [`is_setup_sqpoll`](crate::Parameters::is_setup_sqpoll) is set, whether the kernel
    /// threads has gone to sleep and requires a system call to wake it up.
    #[inline]
    pub fn need_wakeup(&self) -> bool {
        self.queue.need_wakeup()
    }

    /// The number of invalid submission queue entries that have been encountered in the ring
    /// buffer.
    #[inline]
    pub fn dropped(&self) -> u32 {
        self.queue.dropped()
    }

    /// Get the total number of entries in the submission queue ring buffer.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.ring_entries as usize
    }

    /// Get the number of submission queue events in the ring buffer.
    #[inline]
    pub fn len(&self) -> usize {
        unsafe {
            let head = (*self.queue.head).load(atomic::Ordering::Acquire);
            let tail = self.reserved_tail.load(atomic::Ordering::Acquire);

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

    /// Attempts to push an [`Entry`] into the queue.
    /// If the queue is full, the element is returned back as an error.
    ///
    /// # Safety
    ///
    /// Developers must ensure that parameters of the [`Entry`] (such as buffer) are valid and will
    /// be valid for the entire duration of the operation, otherwise it may cause memory problems.
    pub unsafe fn push(&self, Entry(entry): Entry) -> Result<(), Entry> {
        let head = (*self.queue.head).load(atomic::Ordering::Acquire);

        let previous_reserved_tail = self
            .reserved_tail
            .fetch_update(
                atomic::Ordering::Acquire,
                atomic::Ordering::Relaxed,
                |tail| {
                    if tail.wrapping_sub(head) == self.ring_entries {
                        None
                    } else {
                        Some(tail.wrapping_add(1))
                    }
                },
            )
            .map_err(|_| Entry(entry))?;

        *self
            .queue
            .sqes
            .add((previous_reserved_tail & self.ring_mask) as usize) = entry;

        while (*self.queue.tail)
            .compare_exchange(
                previous_reserved_tail,
                previous_reserved_tail.wrapping_add(1),
                atomic::Ordering::Release,
                atomic::Ordering::Relaxed,
            )
            .is_err()
        {
            libc::syscall(
                libc::SYS_futex,
                self.queue.tail,
                libc::FUTEX_WAIT,
                previous_reserved_tail,
                // No timeout.
                std::ptr::null::<libc::timespec>(),
                // Ignored by FUTEX_WAIT.
                std::ptr::null::<u32>(),
                // Ignored by FUTEX_WAIt.
                0_u32,
            );
        }

        libc::syscall(
            libc::SYS_futex,
            self.queue.tail,
            libc::FUTEX_WAKE,
            i32::MAX,
            // Ignored by FUTEX_WAKE.
            std::ptr::null::<libc::timespec>(),
            // Ignored by FUTEX_WAKE.
            std::ptr::null::<u32>(),
            // Ignored by FUTEX_WAKE.
            0_u32,
        );

        Ok(())
    }
}
