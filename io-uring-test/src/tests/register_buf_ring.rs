#[cfg(feature = "unstable")]
use crate::Test;
#[cfg(feature = "unstable")]
use io_uring::types;
#[cfg(feature = "unstable")]
use io_uring::types::io_uring_buf;
#[cfg(feature = "unstable")]
use io_uring::{cqueue, opcode, squeue, IoUring};

#[cfg(feature = "unstable")]
use std::cell::Cell;
#[cfg(feature = "unstable")]
use std::io;
#[cfg(feature = "unstable")]
use std::os::unix::io::AsRawFd;
#[cfg(feature = "unstable")]
use std::rc::Rc;
#[cfg(feature = "unstable")]
use std::sync::atomic::{self, AtomicU16};

#[cfg(feature = "unstable")]
type Bgid = u16;
#[cfg(feature = "unstable")]
type Bid = u16;

#[allow(dead_code)]
#[cfg(feature = "unstable")]
struct BRInner {
    bgid: Bgid,

    ring_entries_mask: u16, // Invariant one less than ring_entries which is > 0, power of 2, max 2^15 (32768).

    buf_cnt: u16,   // Invariants: > 0, <= ring_entries.
    buf_len: usize, // Invariant: > 0.
    layout: std::alloc::Layout,
    ptr: *mut u8,     // Invariant: non zero.
    buf_ptr: *mut u8, // Invariant: non zero.
    local_tail: Cell<u16>,
    br_tail: *const AtomicU16,
}

#[derive(Debug, Copy, Clone)]
// Build the arguments to call build() that returns a BufRing.
// Refer to the methods descriptions for details.
#[allow(dead_code)]
#[cfg(feature = "unstable")]
struct Builder {
    bgid: Bgid,
    ring_entries: u16,
    buf_cnt: u16,
    buf_len: usize,
}

#[allow(dead_code)]
#[cfg(feature = "unstable")]
impl Builder {
    // Create a new Builder with the given buffer group ID and defaults.
    //
    // The buffer group ID, `bgid`, is the id the kernel uses to identify the buffer group to use
    // for a given read operation that has been placed into an sqe.
    //
    // The caller is responsible for picking a bgid that does not conflict with other buffer
    // groups that have been registered with the same uring interface.
    fn new(bgid: Bgid) -> Builder {
        Builder {
            bgid,
            ring_entries: 128,
            buf_cnt: 0,
            buf_len: 4096,
        }
    }

    // The number of ring entries to create for the buffer ring.
    //
    // The number will be made a power of 2, and will be the maximum of the ring_entries setting
    // and the buf_cnt setting. The interface will enforce a maximum of 2^15 (32768) so it can do
    // rollover calculation.
    fn ring_entries(mut self, ring_entries: u16) -> Builder {
        self.ring_entries = ring_entries;
        self
    }

    // The number of buffers to allocate. If left zero, the ring_entries value will be used.
    fn buf_cnt(mut self, buf_cnt: u16) -> Builder {
        self.buf_cnt = buf_cnt;
        self
    }

    // The length to be preallocated for each buffer.
    fn buf_len(mut self, buf_len: usize) -> Builder {
        self.buf_len = buf_len;
        self
    }

    // Skip automatic registration. The caller can manually invoke the buf_ring.register()
    // function later. Regardless, the unregister() method will be called automatically when the
    // BufRing goes out of scope if the caller hadn't manually called buf_ring.unregister()
    // already.
    // Return a BufRing, having computed the layout for the single aligned allocation
    // of both the buffer ring elements and the buffers themselves.
    fn build(&self) -> io::Result<FixedBufRing> {
        let mut b: Builder = *self;

        // Two cases where both buf_cnt and ring_entries are set to the max of the two.
        if b.buf_cnt == 0 || b.ring_entries < b.buf_cnt {
            let max = std::cmp::max(b.ring_entries, b.buf_cnt);
            b.buf_cnt = max;
            b.ring_entries = max;
        }

        // Don't allow the next_power_of_two calculation to be done if already larger than 2^15
        // because 2^16 reads back as 0 in a u16. And the interface doesn't allow for ring_entries
        // larger than 2^15 anyway, so this is a good place to catch it. Here we return a unique
        // error that is more descriptive than the InvalidArg that would come from the interface.
        if b.ring_entries > (1 << 15) {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "ring_entries exceeded 32768",
            ));
        }

        // Requirement of the interface is the ring entries is a power of two, making its and our
        // mask calculation trivial.
        b.ring_entries = b.ring_entries.next_power_of_two();

        let inner = BRInner::new(b.bgid, b.ring_entries, b.buf_cnt, b.buf_len)?;
        Ok(FixedBufRing::new(inner))
    }
}

#[cfg(feature = "unstable")]
impl BRInner {
    #[allow(clippy::too_many_arguments)]
    fn new(bgid: Bgid, ring_entries: u16, buf_cnt: u16, buf_len: usize) -> io::Result<BRInner> {
        #[allow(non_upper_case_globals)]
        // Check that none of the important args are zero and the ring_entries is at least large
        // enough to hold all the buffers and that ring_entries is a power of 2.
        if (buf_cnt == 0)
            || (buf_cnt > ring_entries)
            || (buf_len == 0)
            || ((ring_entries & (ring_entries - 1)) != 0)
        {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }

        // entry_size is 64 bytes.
        let entry_size = std::mem::size_of::<io_uring_buf>() as usize;
        let ring_size = entry_size * (ring_entries as usize);
        let buf_size = buf_len * (buf_cnt as usize);
        let tot_size: usize = ring_size + buf_size;
        let align: usize = 4096; // alignment should be the page size
        let align = align.next_power_of_two();
        let layout = std::alloc::Layout::from_size_align(tot_size, align).unwrap();

        let ptr: *mut u8 = unsafe { std::alloc::alloc_zeroed(layout) };
        // Buffers starts after the ring_size.
        let buf_ptr: *mut u8 = unsafe { ptr.add(ring_size) };

        let br_tail = unsafe { ptr.add(types::RING_BUFFER_TAIL_OFFSET) } as *const AtomicU16;

        let ring_entries_mask = ring_entries - 1;
        assert!((ring_entries & ring_entries_mask) == 0);

        let buf_ring = BRInner {
            bgid,
            ring_entries_mask,
            buf_cnt,
            buf_len,
            layout,
            ptr,
            buf_ptr,
            local_tail: Cell::new(0),
            br_tail,
        };

        Ok(buf_ring)
    }

    // Register the buffer ring with the uring interface.
    // Normally this is done automatically when building a BufRing.
    //
    // Warning: requires the CURRENT driver is already in place or will panic.
    #[allow(dead_code)]
    fn register<S, C>(&self, ring: &mut IoUring<S, C>) -> io::Result<()>
    where
        S: squeue::EntryMarker,
        C: cqueue::EntryMarker,
    {
        let bgid = self.bgid;

        let res = ring
            .submitter()
            .register_buf_ring(self.ptr as _, self.ring_entries(), bgid);

        if let Err(e) = res {
            match e.raw_os_error() {
                Some(22) => {
                    // using buf_ring requires kernel 5.19 or greater.
                    return Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!("buf_ring.register returned {}, most likely indicating this kernel is not 5.19+", e),
                            ));
                }
                Some(17) => {
                    // Registering a duplicate bgid is not allowed. There is an `unregister`
                    // operations that can remove the first, but care must be taken that there
                    // are no outstanding operations that will still return a buffer from that
                    // one.
                    return Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!(
                                "buf_ring.register returned `{}`, indicating the attempted buffer group id {} was already registered",
                            e,
                            bgid),
                        ));
                }
                _ => {
                    eprintln!("buf_ring.register returned `{}` for group id {}", e, bgid);
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("buf_ring.register returned `{}` for group id {}", e, bgid),
                    ));
                }
            }
        };

        // Add the buffers after the registration. Really seems it could be done earlier too.

        for bid in 0..self.buf_cnt {
            self.buf_ring_add(bid);
        }
        self.buf_ring_sync();

        res
    }

    // Unregister the buffer ring from the io_uring.
    // Normally this is done automatically when the BufRing goes out of scope.
    #[allow(dead_code)]
    fn unregister<S, C>(&self, ring: &mut IoUring<S, C>) -> io::Result<()>
    where
        S: squeue::EntryMarker,
        C: cqueue::EntryMarker,
    {
        let bgid = self.bgid;

        ring.submitter().unregister_buf_ring(bgid)
    }

    /// Returns the buffer group id.
    #[inline]
    #[allow(dead_code)]
    fn bgid(&self) -> Bgid {
        self.bgid
    }

    #[allow(dead_code)]
    fn get_buf(&self, res: u32, flags: u32, buf_ring: FixedBufRing) -> io::Result<BufX> {
        // This fn does the odd thing of having self as the BufRing and taking an argument that is
        // the same BufRing but wrapped in Rc<_> so the wrapped buf_ring can be passed to the
        // outgoing BufX.

        let bid = io_uring::cqueue::buffer_select(flags).unwrap();

        let len = res as usize;

        assert!(len <= self.buf_len);

        Ok(BufX::new(buf_ring, bid, len))
    }

    // Safety: dropping a duplicate bid is likely to cause undefined behavior
    // as the kernel could use the same buffer for different data concurrently.
    unsafe fn dropping_bid(&self, bid: Bid) {
        self.buf_ring_add(bid);
        self.buf_ring_sync();
    }

    #[inline]
    fn buf_capacity(&self) -> usize {
        self.buf_len as _
    }

    #[inline]
    fn stable_ptr(&self, bid: Bid) -> *const u8 {
        assert!(bid < self.buf_cnt);
        let offset: usize = self.buf_len * (bid as usize);
        unsafe { self.buf_ptr.add(offset) }
    }

    #[inline]
    fn ring_entries(&self) -> u16 {
        self.ring_entries_mask + 1
    }

    #[inline]
    fn mask(&self) -> u16 {
        self.ring_entries_mask
    }

    // Assign 'buf' with the addr/len/buffer ID supplied
    fn buf_ring_add(&self, bid: Bid) {
        assert!(bid < self.buf_cnt);

        let buf = self.ptr as *mut io_uring_buf;
        let old_tail = self.local_tail.get();
        let new_tail = old_tail + 1;
        self.local_tail.set(new_tail);
        let ring_idx = old_tail & self.mask();

        let buf = unsafe { &mut *buf.add(ring_idx as usize) };

        buf.addr = self.stable_ptr(bid) as _;
        buf.len = self.buf_len as _;
        buf.bid = bid;
        /* most interesting
        println!(
            "{}:{}: buf_ring_add {:?} old_tail {old_tail} new_tail {new_tail} ring_idx {ring_idx}",
            file!(),
            line!(),
            buf
        );
        */
    }

    // Make 'count' new buffers visible to the kernel. Called after
    // io_uring_buf_ring_add() has been called 'count' times to fill in new
    // buffers.
    #[inline]
    fn buf_ring_sync(&self) {
        /*
        println!(
            "{}:{}: sync stores tail {}",
            file!(),
            line!(),
            self.local_tail.get()
        );
        */
        unsafe {
            //atomic::fence(atomic::Ordering::SeqCst);
            (*self.br_tail).store(self.local_tail.get(), atomic::Ordering::Release);
        }

        // TODO figure out if this is needed, io_uring_smp_store_release(&br.tail, local_tail);
    }
}

#[cfg(feature = "unstable")]
impl Drop for BRInner {
    fn drop(&mut self) {
        /*
         * This test version of BufRing does not unregister automatically when being dropped.
         * To do so, would probably require tracking a registered field after all.
        if self.registered.get() {
            _ = self.unregister();
        }
        */
        unsafe { std::alloc::dealloc(self.ptr, self.layout) };
    }
}

// TODO rename so BRInner becomes BufRingI
// and then FixedBufRing can become FixedBufRing.
/// TODO
#[derive(Clone)]
#[cfg(feature = "unstable")]
struct FixedBufRing {
    rc: Rc<BRInner>,
}

#[cfg(feature = "unstable")]
impl FixedBufRing {
    fn new(buf_ring: BRInner) -> Self {
        FixedBufRing {
            rc: Rc::new(buf_ring),
        }
    }
}

// This tracks a buffer that has been filled in by the kernel, having gotten the memory
// from a buffer ring, and returned to userland via a cqe entry.
#[cfg(feature = "unstable")]
struct BufX {
    bgroup: FixedBufRing,
    len: usize,
    bid: Bid,
}

#[cfg(feature = "unstable")]
impl BufX {
    fn new(bgroup: FixedBufRing, bid: Bid, len: usize) -> Self {
        assert!(len <= bgroup.rc.buf_len);

        Self { bgroup, len, bid }
    }

    /// Return the number of bytes initialized.
    ///
    /// This value initially came from the kernel, as reported in the cqe. This value may have been
    /// modified with a call to the IoBufMut::set_init method.
    #[inline]
    #[allow(dead_code)]
    fn len(&self) -> usize {
        self.len as _
    }

    /// Return true if this represents an empty buffer. The length reported by the kernel was 0.
    #[inline]
    #[allow(dead_code)]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Return the capacity of this buffer.
    #[inline]
    #[allow(dead_code)]
    fn cap(&self) -> usize {
        self.bgroup.rc.buf_capacity()
    }

    /// Return a byte slice reference.
    #[inline]
    fn as_slice(&self) -> &[u8] {
        let p = self.bgroup.rc.stable_ptr(self.bid);
        unsafe { std::slice::from_raw_parts(p, self.len) }
    }
}

#[cfg(feature = "unstable")]
impl Drop for BufX {
    fn drop(&mut self) {
        // Add the buffer back to the bgroup, for the kernel to reuse.
        unsafe { self.bgroup.rc.dropping_bid(self.bid) };
    }
}

#[cfg(feature = "unstable")]
fn reg_and_unreg<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    _test: &Test,
) -> anyhow::Result<()> {
    _ = ring;
    // Create a BufRing
    // Register it
    // Unregister it
    //
    // Register it
    // Try to register it again
    // Unregister it
    // Try to unnregister it again
    // Drop it

    let bgid = 777;
    let ring_entries = 1;
    let buf_cnt = 1;
    let buf_len = 128;
    let buf_ring = Builder::new(bgid)
        .ring_entries(ring_entries)
        .buf_cnt(buf_cnt)
        .buf_len(buf_len)
        .build()?;

    buf_ring.rc.register(ring)?;
    buf_ring.rc.unregister(ring)?;

    buf_ring.rc.register(ring)?;
    assert!(buf_ring.rc.register(ring).is_err());
    buf_ring.rc.unregister(ring)?;
    assert!(buf_ring.rc.unregister(ring).is_err());

    Ok(())
}

// Following utils::write_read example.
#[cfg(feature = "unstable")]
fn write_read_with_buf<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    fd_in: types::Fd,
    fd_out: types::Fd,
    buf_ring: &FixedBufRing,
) -> anyhow::Result<()> {
    let text = b"The quick brown fox jumps over the lazy dog.";

    let write_e = opcode::Write::new(fd_in, text.as_ptr(), text.len() as _);
    let read_e = opcode::Read::new(fd_out, std::ptr::null_mut(), 0)
        .offset(0)
        .buf_group(buf_ring.rc.bgid());

    unsafe {
        let mut queue = ring.submission();
        let write_e = write_e
            .build()
            .user_data(0x01)
            .flags(squeue::Flags::IO_LINK)
            .into();
        queue.push(&write_e).expect("queue is full");
    }
    assert_eq!(ring.submit_and_wait(1)?, 1);
    {
        let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

        assert_eq!(cqes.len(), 1);
        assert_eq!(cqes[0].user_data(), 0x01);
        assert_eq!(cqes[0].result(), text.len() as i32);
    }

    unsafe {
        let mut queue = ring.submission();
        queue
            .push(
                &read_e
                    .build()
                    .user_data(0x02)
                    .flags(squeue::Flags::BUFFER_SELECT)
                    .into(),
            )
            .expect("queue is full");
    }
    assert_eq!(ring.submit_and_wait(1)?, 1);
    {
        let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

        assert_eq!(cqes.len(), 1);
        assert_eq!(cqes[0].user_data(), 0x02);

        let result = cqes[0].result();
        assert!(result >= 0);

        // After these low level checks, return the cqueue::Entry
        // and let higher level deal with the buf_ring aspect.
        // Or instead of buf_ring, get buf pointer
        // and then use length to unsafely create a slice right here to be tested.

        let flags = cqes[0].flags();
        let len = result as usize;
        let bid = cqueue::buffer_select(flags).unwrap();
        assert_eq!(bid, 0); // To change to something more unique.
        assert_eq!(len, text.len());

        let bufx = buf_ring
            .rc
            .get_buf(result as u32, flags, buf_ring.clone())?;

        // Verify data put into the ring_buf buffer.
        assert_eq!(bufx.as_slice(), text);
    }

    Ok(())
}

#[cfg(feature = "unstable")]
fn actually_use_buffer<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    _test: &Test,
) -> anyhow::Result<()> {
    // Much more advanced:

    // Use with a temporary file that has a line it.
    // Check that the buf ring can be used to fill in data from a read.

    let bgid = 888;
    let ring_entries = 1;
    let buf_cnt = 1;
    let buf_len = 128;
    let buf_ring = Builder::new(bgid)
        .ring_entries(ring_entries)
        .buf_cnt(buf_cnt)
        .buf_len(buf_len)
        .build()?;

    buf_ring.rc.register(ring)?;

    let fd = tempfile::tempfile()?;
    let fd = types::Fd(fd.as_raw_fd());
    write_read_with_buf(ring, fd, fd, &buf_ring)?;

    buf_ring.rc.unregister(ring)?;

    Ok(())
}

#[cfg(feature = "unstable")]
pub fn test_register_buf_ring<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    // register_buf_ring was introduced in kernel 5.19, as was the opcode for UringCmd16.
    // So require the UringCmd16 to avoid running this test on earlier kernels.
    // Another option for the test or an application is to check for one of the 5.19 features
    // although sadly, there is no feature that specifically targets registering a buf_ring.
    // An application can always just try to the register_buf_ring and determine support by
    // checking the result for success.
    require!(
        test;
        test.probe.is_supported(opcode::UringCmd16::CODE);
    );

    println!("test register_buf_ring");

    reg_and_unreg(ring, test)?;

    actually_use_buffer(ring, test)?;

    Ok(())
}
