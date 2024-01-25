use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut};
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
                let buf = unsafe { s._recycle_by_index(i) };
                buf.len = entry_size;
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

impl<'a> BufRingSubmissions<'a> {
    pub fn sync(&mut self) {
        unsafe { &*self.tail_ptr }.store(self.tail.0 as u16, atomic::Ordering::Release);
    }

    pub unsafe fn get(&mut self, flags: u32, len: usize) -> Buf<'_, 'a> {
        let index = Self::flags_to_index(flags);
        let buf = unsafe { self.buf_ptr.add(usize::from(index) * self.entry_size) };
        Buf {
            buf: unsafe { slice::from_raw_parts_mut(buf.cast(), len) },
            index,
            submissions: self,
        }
    }

    pub unsafe fn recycle(&mut self, flags: u32) {
        self.recycle_by_index(Self::flags_to_index(flags));
    }

    pub unsafe fn recycle_by_index(&mut self, index: u16) {
        self._recycle_by_index(index);
    }

    unsafe fn _recycle_by_index(&mut self, index: u16) -> &mut sys::io_uring_buf {
        let uindex = usize::from(index);
        {
            let next_buf = unsafe { &mut *self.ring_ptr.add(self.tail.0 & self.tail_mask) };
            next_buf.addr = unsafe { self.buf_ptr.add(uindex * self.entry_size) } as u64;
            next_buf.bid = index;
        }
        self.tail += Wrapping(1);

        unsafe { &mut *self.ring_ptr.add(uindex) }
    }

    fn flags_to_index(flags: u32) -> u16 {
        u16::try_from(flags >> sys::IORING_CQE_BUFFER_SHIFT).unwrap()
    }
}

impl Drop for BufRingSubmissions<'_> {
    fn drop(&mut self) {
        self.sync()
    }
}

pub struct Buf<'a, 'b> {
    buf: &'a mut [u8],
    index: u16,
    submissions: &'a mut BufRingSubmissions<'b>,
}

impl Deref for Buf<'_, '_> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.buf
    }
}

impl DerefMut for Buf<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.buf
    }
}

impl Buf<'_, '_> {
    pub fn into_index(self) -> u16 {
        let me = ManuallyDrop::new(self);
        me.index
    }
}

impl Drop for Buf<'_, '_> {
    fn drop(&mut self) {
        unsafe {
            self.submissions.recycle_by_index(self.index);
        }
    }
}
