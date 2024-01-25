use std::{convert::TryFrom, marker::PhantomData, num::Wrapping, slice, sync::atomic};

use crate::{sys, util::Mmap};

pub struct BufRing {
    ring: Mmap,
    ring_entries: u16,
    entry_size: u32,
}

impl BufRing {
    pub(crate) fn init(ring: Mmap, ring_entries: u16, entry_size: u32) -> Self {
        let mut this = Self {
            ring,
            ring_entries,
            entry_size,
        };

        {
            let mut s = this.submissions();
            for i in 0u16..ring_entries {
                let buf = unsafe { s.recycle_by_index(usize::from(i)) };
                buf.len = entry_size;
                buf.bid = i;
            }
        }

        this
    }

    pub fn submissions(&mut self) -> BufRingSubmissions<'_> {
        let ring_ptr = self.ring.as_mut_ptr().cast::<sys::io_uring_buf>();
        let tail_ptr = unsafe { self.ring.offset(8 + 4 + 2) };
        let ring_entries = usize::from(self.ring_entries);
        BufRingSubmissions {
            ring_ptr,
            buf_ptr: unsafe { ring_ptr.add(ring_entries).cast() },
            tail_ptr: tail_ptr.cast::<atomic::AtomicU16>(),

            tail: Wrapping(usize::from(unsafe { *tail_ptr.cast::<u16>() })),
            tail_mask: ring_entries - 1,
            entry_size: usize::try_from(self.entry_size).unwrap(),

            _marker: PhantomData,
        }
    }
}

pub struct BufRingSubmissions<'ctx> {
    ring_ptr: *mut sys::io_uring_buf,
    buf_ptr: *mut libc::c_void,
    tail_ptr: *const atomic::AtomicU16,

    tail: Wrapping<usize>,
    tail_mask: usize,
    entry_size: usize,

    _marker: PhantomData<&'ctx ()>,
}

impl BufRingSubmissions<'_> {
    pub fn sync(&mut self) {
        unsafe { &*self.tail_ptr }.store(self.tail.0 as u16, atomic::Ordering::Release);
    }

    pub unsafe fn recycle(&mut self, flags: u32, len: usize) -> &mut [u8] {
        let buf =
            self.recycle_by_index(usize::try_from(flags >> sys::IORING_CQE_BUFFER_SHIFT).unwrap());
        unsafe { slice::from_raw_parts_mut(buf.addr as *mut u8, len) }
    }

    unsafe fn recycle_by_index(&mut self, index: usize) -> &mut sys::io_uring_buf {
        {
            let next_buf = unsafe { &mut *self.ring_ptr.add(self.tail.0 & self.tail_mask) };
            next_buf.addr = unsafe { self.buf_ptr.add(index * self.entry_size) } as u64;
        }
        self.tail += 1;

        unsafe { &mut *self.ring_ptr.add(index) }
    }
}

impl Drop for BufRingSubmissions<'_> {
    fn drop(&mut self) {
        self.sync()
    }
}
