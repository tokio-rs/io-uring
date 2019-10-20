use std::{ io, mem };
use std::sync::atomic;
use bitflags::bitflags;
use linux_io_uring_sys as sys;
use crate::util::{ Mmap, Fd, AtomicU32Ref };
use crate::mmap_offset;


pub struct SubmissionQueue {
    _sq_mmap: Mmap,
    _sqe_mmap: Mmap,

    head: *const atomic::AtomicU32,
    tail: AtomicU32Ref,
    ring_mask: *const u32,
    ring_entries: *const u32,
    flags: *const atomic::AtomicU32,
    dropped: *const atomic::AtomicU32,

    #[allow(dead_code)]
    array: *mut u32,

    sqes: *mut sys::io_uring_sqe
}

pub struct AvailableQueue<'a> {
    head: u32,
    tail: u32,
    ring_mask: u32,
    ring_entries: u32,
    queue: &'a mut SubmissionQueue
}

#[derive(Clone)]
pub struct Entry(pub(crate) sys::io_uring_sqe);

bitflags!{
    pub struct Flags: u8 {
        const FIXED_FILE = sys::IOSQE_FIXED_FILE as _;
        const IO_DRAIN = sys::IOSQE_IO_DRAIN as _;
        const IO_LINK = sys::IOSQE_IO_LINK as _;
    }
}

impl SubmissionQueue {
    pub(crate) fn new(fd: &Fd, p: &sys::io_uring_params) -> io::Result<SubmissionQueue> {
        let sq_mmap = Mmap::new(
            &fd,
            sys::IORING_OFF_SQ_RING as _,
            p.sq_off.array as usize + p.sq_entries as usize * mem::size_of::<u32>()
        )?;
        let sqe_mmap = Mmap::new(
            &fd,
            sys::IORING_OFF_SQES as _,
            p.sq_entries as usize * mem::size_of::<sys::io_uring_sqe>()
        )?;

        mmap_offset!{ unsafe
            let head            = sq_mmap + p.sq_off.head           => *const atomic::AtomicU32;
            let tail            = sq_mmap + p.sq_off.tail           => *const u32;
            let ring_mask       = sq_mmap + p.sq_off.ring_mask      => *const u32;
            let ring_entries    = sq_mmap + p.sq_off.ring_entries   => *const u32;
            let flags           = sq_mmap + p.sq_off.flags          => *const atomic::AtomicU32;
            let dropped         = sq_mmap + p.sq_off.dropped        => *const atomic::AtomicU32;
            let array           = sq_mmap + p.sq_off.array          => *mut u32;

            let sqes            = sqe_mmap + 0                      => *mut sys::io_uring_sqe;
        }

        unsafe {
            // To keep it simple, map it directly to `sqes`.
            for i in 0..*ring_entries {
                *array.add(i as usize) = i;
            }
        }

        Ok(SubmissionQueue {
            _sq_mmap: sq_mmap,
            _sqe_mmap: sqe_mmap,
            head,
            tail: unsafe { AtomicU32Ref::new(tail) },
            ring_mask, ring_entries,
            flags, dropped,
            array,
            sqes
        })
    }

    pub fn need_wakeup(&self) -> bool {
        unsafe {
            (*self.flags).load(atomic::Ordering::Acquire) & sys::IORING_SQ_NEED_WAKEUP
                == 0
        }
    }

    pub fn dropped(&self) -> u32 {
        unsafe {
            (*self.dropped).load(atomic::Ordering::Acquire)
        }
    }

    pub fn len(&self) -> usize {
        let head = unsafe { (*self.head).load(atomic::Ordering::Acquire) };
        let tail = self.tail.unsync_load();

        tail.wrapping_sub(head) as usize
    }

    pub fn is_empty(&self) -> bool {
        let head = unsafe { (*self.head).load(atomic::Ordering::Acquire) };
        let tail = self.tail.unsync_load();

        head == tail
    }

    pub fn is_full(&self) -> bool {
        let head = unsafe { (*self.head).load(atomic::Ordering::Acquire) };
        let tail = self.tail.unsync_load();
        let ring_entries = unsafe { *self.ring_entries };

        tail.wrapping_sub(head) == ring_entries
    }

    pub fn available(&mut self) -> AvailableQueue<'_> {
        unsafe {
            AvailableQueue {
                head: (*self.head).load(atomic::Ordering::Acquire),
                tail: self.tail.unsync_load(),
                ring_mask: *self.ring_mask,
                ring_entries: *self.ring_entries,
                queue: self
            }
        }
    }
}

impl AvailableQueue<'_> {
    pub fn len(&self) -> usize {
        self.tail.wrapping_sub(self.head) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    pub fn is_full(&self) -> bool {
        self.tail.wrapping_sub(self.head) == self.ring_entries
    }

    pub unsafe fn push(&mut self, Entry(entry): Entry) -> Result<(), Entry> {
        if self.is_full() {
            Err(Entry(entry))
        } else {
            *self.queue.sqes.add((self.tail & self.ring_mask) as usize)
                = entry;
            self.tail = self.tail.wrapping_add(1);
            Ok(())
        }
    }
}

impl Drop for AvailableQueue<'_> {
    fn drop(&mut self) {
        self.queue.tail.store(self.tail, atomic::Ordering::Release);
    }
}

impl Entry {
    pub fn flags(mut self, flags: Flags) -> Entry {
        self.0.flags |= flags.bits();
        self
    }

    pub fn ioprio(mut self, ioprio: u16) -> Entry {
        self.0.ioprio = ioprio;
        self
    }

    pub fn user_data(mut self, user_data: u64) -> Entry {
        self.0.user_data = user_data;
        self
    }
}
