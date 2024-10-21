use std::{
    io,
    mem::{self, MaybeUninit},
    os::fd::{AsRawFd, RawFd},
    ptr, slice,
    sync::atomic::{AtomicU16, Ordering},
};

use crate::{
    register::execute,
    sys,
    types::BufRingEntry,
    util::{cast_ptr, OwnedFd},
};

pub(crate) fn register(fd: RawFd, ring_addr: u64, ring_entries: u16, bgid: u16) -> io::Result<()> {
    // The interface type for ring_entries is u32 but the same interface only allows a u16 for
    // the tail to be specified, so to try and avoid further confusion, we limit the
    // ring_entries to u16 here too. The value is actually limited to 2^15 (32768) but we can
    // let the kernel enforce that.
    let arg = sys::io_uring_buf_reg {
        ring_addr,
        ring_entries: ring_entries as _,
        bgid,
        ..Default::default()
    };
    execute(
        fd,
        sys::IORING_REGISTER_PBUF_RING,
        cast_ptr::<sys::io_uring_buf_reg>(&arg).cast(),
        1,
    )
    .map(drop)
}

pub(crate) fn unregister(fd: RawFd, bgid: u16) -> io::Result<()> {
    let arg = sys::io_uring_buf_reg {
        ring_addr: 0,
        ring_entries: 0,
        bgid,
        ..Default::default()
    };
    execute(
        fd,
        sys::IORING_UNREGISTER_PBUF_RING,
        cast_ptr::<sys::io_uring_buf_reg>(&arg).cast(),
        1,
    )
    .map(drop)
}

/// An anonymous region of memory mapped using `mmap(2)`, not backed by a file
/// but that is guaranteed to be page-aligned and zero-filled.
struct AnonymousMmap {
    addr: ptr::NonNull<libc::c_void>,
    len: usize,
}

impl AnonymousMmap {
    /// Allocate `len` bytes that are page aligned and zero-filled.
    pub fn new(len: usize) -> io::Result<AnonymousMmap> {
        unsafe {
            match libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_ANONYMOUS | libc::MAP_SHARED | libc::MAP_POPULATE,
                -1,
                0,
            ) {
                libc::MAP_FAILED => Err(io::Error::last_os_error()),
                addr => {
                    // here, `mmap` will never return null
                    let addr = ptr::NonNull::new_unchecked(addr);
                    Ok(AnonymousMmap { addr, len })
                }
            }
        }
    }

    /// Do not make the stored memory accessible by child processes after a `fork`.
    pub fn dontfork(&self) -> io::Result<()> {
        match unsafe { libc::madvise(self.addr.as_ptr(), self.len, libc::MADV_DONTFORK) } {
            0 => Ok(()),
            _ => Err(io::Error::last_os_error()),
        }
    }

    /// Get a pointer to the memory.
    #[inline]
    pub fn as_ptr(&self) -> *const libc::c_void {
        self.addr.as_ptr()
    }

    /// Get a mut pointer to the memory.
    #[inline]
    pub fn as_ptr_mut(&self) -> *mut libc::c_void {
        self.addr.as_ptr()
    }
}

impl Drop for AnonymousMmap {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.addr.as_ptr(), self.len);
        }
    }
}

/// A wrapper for buffer ring. The entries are allocated with `mmap`.
///
/// This struct doesn't own the buffer.
pub struct BufRing<'a> {
    fd: &'a OwnedFd,
    entries: mem::ManuallyDrop<AnonymousMmap>,
    len: u16,
    bgid: u16,
}

impl<'a> BufRing<'a> {
    pub(crate) fn new(fd: &'a OwnedFd, len: u16, bgid: u16) -> io::Result<Self> {
        let entries = AnonymousMmap::new((len as usize) * mem::size_of::<BufRingEntry>())?;
        entries.dontfork()?;
        register(fd.as_raw_fd(), entries.as_ptr() as _, len, bgid)?;
        // SAFETY: no one use the tail at this moment
        unsafe {
            *BufRingEntry::tail(entries.as_ptr().cast()).cast_mut() = 0;
        }

        Ok(Self {
            fd,
            entries: mem::ManuallyDrop::new(entries),
            len,
            bgid,
        })
    }

    /// Unregister the buffer ring.
    ///
    /// If it fails to unregister, the inner memory will be leaked.
    pub fn unregister(mut self) -> io::Result<()> {
        unregister(self.fd.as_raw_fd(), self.bgid)?;
        // SAFETY: unregister successfully
        unsafe {
            mem::ManuallyDrop::drop(&mut self.entries);
        }
        mem::forget(self);
        Ok(())
    }

    /// Get the number of allocated entries.
    #[inline]
    pub const fn capacity(&self) -> usize {
        self.len as _
    }

    /// Get the buffer group id of current ring.
    #[inline]
    pub const fn bgid(&self) -> u16 {
        self.bgid
    }

    fn as_slice_uninit_mut(&mut self) -> &mut [MaybeUninit<BufRingEntry>] {
        // SAFETY: the pointer is valid
        unsafe { slice::from_raw_parts_mut(self.entries.as_ptr_mut().cast(), self.capacity()) }
    }

    #[inline]
    const fn mask(&self) -> u16 {
        self.len - 1
    }

    #[inline]
    fn atomic_tail(&self) -> &AtomicU16 {
        // Safety: no one read/write tail ptr without atomic operation after init
        unsafe { AtomicU16::from_ptr(BufRingEntry::tail(self.entries.as_ptr().cast()).cast_mut()) }
    }

    unsafe fn push_inner(&mut self, bid: u16, buf: &mut [MaybeUninit<u8>], offset: u16) {
        let mask = self.mask();
        let tail = self.atomic_tail();
        let index = ((tail.load(Ordering::Acquire) + offset) & mask) as usize;

        // SAFETY: only write plain data here
        let buf_ring_entry = self.as_slice_uninit_mut()[index].assume_init_mut();

        buf_ring_entry.set_addr(buf.as_mut_ptr() as _);
        buf_ring_entry.set_len(buf.len() as _);
        buf_ring_entry.set_bid(bid);
    }

    unsafe fn advance(&self, count: u16) {
        self.atomic_tail().fetch_add(count, Ordering::Release);
    }

    /// Attempts to push an buffer entry into the ring.
    ///
    /// # Safety
    ///
    /// Developers must ensure that the buffer is valid before the ring is unregistered.
    pub unsafe fn push(&mut self, bid: u16, buf: &mut [MaybeUninit<u8>]) {
        self.push_inner(bid, buf, 0);
        self.advance(1);
    }

    /// Attempts to push several buffer entries into the ring.
    ///
    /// # Safety
    ///
    /// Developers must ensure that the buffers are valid before the ring is unregistered.
    pub unsafe fn push_multiple<'b>(
        &mut self,
        bufs: impl IntoIterator<Item = (u16, &'b mut [MaybeUninit<u8>])>,
    ) {
        let mut len = 0u16;
        for (i, (bid, buf)) in bufs.into_iter().enumerate() {
            self.push_inner(bid, buf, i as _);
            len += 1;
        }
        self.advance(len);
    }
}

impl Drop for BufRing<'_> {
    fn drop(&mut self) {
        // SAFETY: ManuallyDrop
        unsafe {
            ptr::read(self).unregister().ok();
        }
    }
}
