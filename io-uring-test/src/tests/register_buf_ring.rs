// Create tests for registering buf_rings and for the buf_ring entries.
// The entry point in this file can be found by searching for 'pub'.

use crate::Test;
use io_uring::types;
use io_uring::types::BufRingEntry;
use io_uring::{cqueue, opcode, squeue, IoUring};

use std::cell::Cell;
use std::fmt;
use std::io;
use std::os::unix::io::AsRawFd;
use std::ptr;
use std::rc::Rc;
use std::sync::atomic::{self, AtomicU16};

type Bgid = u16; // Buffer group id
type Bid = u16; // Buffer id

/// An anonymous region of memory mapped using `mmap(2)`, not backed by a file
/// but that is guaranteed to be page-aligned and zero-filled.
pub struct AnonymousMmap {
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

    /// Get a pointer to the data at the given offset.
    #[inline]
    #[allow(dead_code)]
    pub unsafe fn offset(&self, offset: u32) -> *const libc::c_void {
        self.as_ptr().add(offset as usize)
    }

    /// Get a mut pointer to the data at the given offset.
    #[inline]
    #[allow(dead_code)]
    pub unsafe fn offset_mut(&self, offset: u32) -> *mut libc::c_void {
        self.as_ptr_mut().add(offset as usize)
    }
}

impl Drop for AnonymousMmap {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.addr.as_ptr(), self.len);
        }
    }
}

struct InnerBufRing {
    // All these fields are constant once the struct is instantiated except the one of type Cell<u16>.
    bgid: Bgid,

    ring_entries_mask: u16, // Invariant one less than ring_entries which is > 0, power of 2, max 2^15 (32768).

    buf_cnt: u16,   // Invariants: > 0, <= ring_entries.
    buf_len: usize, // Invariant: > 0.

    // `ring_start` holds the memory allocated for the buf_ring, the ring of entries describing
    // the buffers being made available to the uring interface for this buf group id.
    ring_start: AnonymousMmap,

    buf_list: Vec<Vec<u8>>,

    // `local_tail` is the copy of the tail index that we update when a buffer is dropped and
    // therefore its buffer id is released and added back to the ring. It also serves for adding
    // buffers to the ring during init but that's not as interesting.
    local_tail: Cell<u16>,

    // `shared_tail` points to the u16 memory inside the rings that the uring interface uses as the
    // tail field. It is where the application writes new tail values and the kernel reads the tail
    // value from time to time. The address could be computed from ring_start when needed. This
    // might be here for no good reason any more.
    shared_tail: *const AtomicU16,
}

impl InnerBufRing {
    fn new(
        bgid: Bgid,
        ring_entries: u16,
        buf_cnt: u16,
        buf_len: usize,
    ) -> io::Result<InnerBufRing> {
        // Check that none of the important args are zero and the ring_entries is at least large
        // enough to hold all the buffers and that ring_entries is a power of 2.
        if (buf_cnt == 0)
            || (buf_cnt > ring_entries)
            || (buf_len == 0)
            || ((ring_entries & (ring_entries - 1)) != 0)
        {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }

        // entry_size is 16 bytes.
        let entry_size = std::mem::size_of::<BufRingEntry>() as usize;
        assert_eq!(entry_size, 16);
        let ring_size = entry_size * (ring_entries as usize);

        // The memory is required to be page aligned and zero-filled by the uring buf_ring
        // interface. Anonymous mmap promises both of those things.
        // https://man7.org/linux/man-pages/man2/mmap.2.html
        let ring_start = AnonymousMmap::new(ring_size).unwrap();
        ring_start.dontfork()?;

        // Probably some functional way to do this.
        let buf_list: Vec<Vec<u8>> = {
            let mut bp = Vec::with_capacity(buf_cnt as _);
            for _ in 0..buf_cnt {
                bp.push(vec![0; buf_len]);
            }
            bp
        };

        let shared_tail =
            unsafe { types::BufRingEntry::tail(ring_start.as_ptr() as *const BufRingEntry) }
                as *const AtomicU16;

        let ring_entries_mask = ring_entries - 1;
        assert!((ring_entries & ring_entries_mask) == 0);

        let buf_ring = InnerBufRing {
            bgid,
            ring_entries_mask,
            buf_cnt,
            buf_len,
            ring_start,
            buf_list,
            local_tail: Cell::new(0),
            shared_tail,
        };

        Ok(buf_ring)
    }

    // Register the buffer ring with the uring interface.
    // Normally this is done automatically when building a BufRing.
    //
    // Warning: requires the CURRENT driver is already in place or will panic.
    fn register<S, C>(&self, ring: &mut IoUring<S, C>) -> io::Result<()>
    where
        S: squeue::EntryMarker,
        C: cqueue::EntryMarker,
    {
        let bgid = self.bgid;

        let res = ring.submitter().register_buf_ring(
            self.ring_start.as_ptr() as _,
            self.ring_entries(),
            bgid,
        );

        if let Err(e) = res {
            match e.raw_os_error() {
                Some(libc::EINVAL) => {
                    // using buf_ring requires kernel 5.19 or greater.
                    return Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!("buf_ring.register returned {}, most likely indicating this kernel is not 5.19+", e),
                            ));
                }
                Some(libc::EEXIST) => {
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
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("buf_ring.register returned `{}` for group id {}", e, bgid),
                    ));
                }
            }
        };

        // Add the buffers after the registration. Really seems it could be done earlier too.

        for bid in 0..self.buf_cnt {
            self.buf_ring_push(bid);
        }
        self.buf_ring_sync();

        res
    }

    // Unregister the buffer ring from the io_uring.
    // Normally this is done automatically when the BufRing goes out of scope.
    fn unregister<S, C>(&self, ring: &mut IoUring<S, C>) -> io::Result<()>
    where
        S: squeue::EntryMarker,
        C: cqueue::EntryMarker,
    {
        let bgid = self.bgid;

        ring.submitter().unregister_buf_ring(bgid)
    }

    // Returns the buffer group id.
    fn bgid(&self) -> Bgid {
        self.bgid
    }

    // Returns the buffer the uring interface picked from the buf_ring for the completion result
    // represented by the res and flags.
    fn get_buf(&self, buf_ring: FixedSizeBufRing, res: u32, flags: u32) -> io::Result<GBuf> {
        // This fn does the odd thing of having self as the BufRing and taking an argument that is
        // the same BufRing but wrapped in Rc<_> so the wrapped buf_ring can be passed to the
        // outgoing GBuf.

        let bid = io_uring::cqueue::buffer_select(flags).unwrap();

        let len = res as usize;

        assert!(len <= self.buf_len);

        Ok(GBuf::new(buf_ring, bid, len))
    }

    // Safety: dropping a duplicate bid is likely to cause undefined behavior
    // as the kernel could use the same buffer for different data concurrently.
    unsafe fn dropping_bid(&self, bid: Bid) {
        self.buf_ring_push(bid);
        self.buf_ring_sync();
    }

    fn buf_capacity(&self) -> usize {
        self.buf_len as _
    }

    fn stable_ptr(&self, bid: Bid) -> *const u8 {
        self.buf_list[bid as usize].as_ptr()
    }

    fn ring_entries(&self) -> u16 {
        self.ring_entries_mask + 1
    }

    fn mask(&self) -> u16 {
        self.ring_entries_mask
    }

    // Push the `bid` buffer to the buf_ring tail.
    // This test version does not safeguard against a duplicate
    // `bid` being pushed.
    fn buf_ring_push(&self, bid: Bid) {
        assert!(bid < self.buf_cnt);

        // N.B. The uring buf_ring indexing mechanism calls for the tail values to exceed the
        // actual number of ring entries. This allows the uring interface to distinguish between
        // empty and full buf_rings. As a result, the ring mask is only applied to the index used
        // for computing the ring entry, not to the tail value itself.

        let old_tail = self.local_tail.get();
        self.local_tail.set(old_tail + 1);
        let ring_idx = old_tail & self.mask();

        let entries = self.ring_start.as_ptr_mut() as *mut BufRingEntry;
        let re = unsafe { &mut *entries.add(ring_idx as usize) };

        re.set_addr(self.stable_ptr(bid) as _);
        re.set_len(self.buf_len as _);
        re.set_bid(bid);

        // Also note, we have not updated the tail as far as the kernel is concerned.
        // That is done with buf_ring_sync.
    }

    // Make 'local_tail' visible to the kernel. Called after buf_ring_push() has been
    // called to fill in new buffers.
    fn buf_ring_sync(&self) {
        unsafe {
            (*self.shared_tail).store(self.local_tail.get(), atomic::Ordering::Release);
        }
    }
}

#[derive(Clone)]
struct FixedSizeBufRing {
    // The BufRing is reference counted because each buffer handed out has a reference back to its
    // buffer group, or in this case, to its buffer ring.
    rc: Rc<InnerBufRing>,
}

impl FixedSizeBufRing {
    fn new(buf_ring: InnerBufRing) -> Self {
        FixedSizeBufRing {
            rc: Rc::new(buf_ring),
        }
    }
}

// The Builder API for a FixedSizeBufRing.
#[derive(Copy, Clone)]
struct Builder {
    bgid: Bgid,
    ring_entries: u16,
    buf_cnt: u16,
    buf_len: usize,
}

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
            buf_cnt: 0, // 0 indicates buf_cnt is taken from ring_entries
            buf_len: 4096,
        }
    }

    // The number of ring entries to create for the buffer ring.
    //
    // The number will be made a power of 2, and will be the maximum of the ring_entries setting
    // and the buf_cnt setting. The interface will enforce a maximum of 2^15 (32768).
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

    // Return a FixedSizeBufRing.
    fn build(&self) -> io::Result<FixedSizeBufRing> {
        let mut b: Builder = *self;

        // Two cases where both buf_cnt and ring_entries are set to the max of the two.
        if b.buf_cnt == 0 || b.ring_entries < b.buf_cnt {
            let max = std::cmp::max(b.ring_entries, b.buf_cnt);
            b.buf_cnt = max;
            b.ring_entries = max;
        }

        // Don't allow the next_power_of_two calculation to be done if already larger than 2^15
        // because 2^16 reads back as 0 in a u16. The interface doesn't allow for ring_entries
        // larger than 2^15 anyway, so this is a good place to catch it. Here we return a unique
        // error that is more descriptive than the InvalidArg that would come from the interface.
        if b.ring_entries > (1 << 15) {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "ring_entries exceeded 32768",
            ));
        }

        // Requirement of the interface is the ring entries is a power of two, making its and our
        // wrap calculation trivial.
        b.ring_entries = b.ring_entries.next_power_of_two();

        let inner = InnerBufRing::new(b.bgid, b.ring_entries, b.buf_cnt, b.buf_len)?;
        Ok(FixedSizeBufRing::new(inner))
    }
}

// This tracks a buffer that has been filled in by the kernel, having gotten the memory
// from a buffer ring, and returned to userland via a cqe entry.
struct GBuf {
    bufgroup: FixedSizeBufRing,
    len: usize,
    bid: Bid,
}

impl fmt::Debug for GBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GBuf")
            .field("bgid", &self.bufgroup.rc.bgid())
            .field("bid", &self.bid)
            .field("len", &self.len)
            .field("cap", &self.bufgroup.rc.buf_capacity())
            .finish()
    }
}

impl GBuf {
    fn new(bufgroup: FixedSizeBufRing, bid: Bid, len: usize) -> Self {
        assert!(len <= bufgroup.rc.buf_len);

        Self { bufgroup, len, bid }
    }

    // A few methods are kept here despite not being used for unit tests yet. They show a little
    // of the intent.

    // Return the number of bytes initialized.
    //
    // This value initially came from the kernel, as reported in the cqe. This value may have been
    // modified with a call to the IoBufMut::set_init method.
    #[allow(dead_code)]
    fn len(&self) -> usize {
        self.len as _
    }

    // Return true if this represents an empty buffer. The length reported by the kernel was 0.
    #[allow(dead_code)]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // Return the capacity of this buffer.
    #[allow(dead_code)]
    fn cap(&self) -> usize {
        self.bufgroup.rc.buf_capacity()
    }

    // Return a byte slice reference.
    fn as_slice(&self) -> &[u8] {
        let p = self.bufgroup.rc.stable_ptr(self.bid);
        unsafe { std::slice::from_raw_parts(p, self.len) }
    }
}

impl Drop for GBuf {
    fn drop(&mut self) {
        // Add the buffer back to the bufgroup, for the kernel to reuse.
        unsafe { self.bufgroup.rc.dropping_bid(self.bid) };
    }
}

// Begin of test functions.

// Verify register and unregister of a buf_ring.
fn buf_ring_reg_and_unreg<S, C>(ring: &mut IoUring<S, C>, _test: &Test) -> io::Result<()>
where
    S: squeue::EntryMarker,
    C: cqueue::EntryMarker,
{
    // Create a BufRing
    // Register it
    // Unregister it
    //
    // Register it
    // Try to register it again
    // Unregister it
    // Try to unnregister it again
    // Drop it

    let buf_ring = Builder::new(777).ring_entries(16).buf_len(4096).build()?;

    buf_ring.rc.register(ring)?;
    buf_ring.rc.unregister(ring)?;

    // Register twice.
    buf_ring.rc.register(ring)?;
    assert!(buf_ring.rc.register(ring).is_err());

    // Unregister twice.
    buf_ring.rc.unregister(ring)?;
    assert!(buf_ring.rc.unregister(ring).is_err());

    Ok(())
}

// Write sample to file descriptor.
fn write_text_to_file<S, C>(ring: &mut IoUring<S, C>, fd: types::Fd, text: &[u8]) -> io::Result<()>
where
    S: squeue::EntryMarker,
    C: cqueue::EntryMarker,
{
    let write_e = opcode::Write::new(fd, text.as_ptr(), text.len() as _);

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

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x01);
    assert_eq!(cqes[0].result(), text.len() as i32);
    Ok(())
}

// Read from file descriptor, returning a buffer from the buf_ring.
fn buf_ring_read<S, C>(
    ring: &mut IoUring<S, C>,
    buf_ring: &FixedSizeBufRing,
    fd: types::Fd,
    len: u32,
) -> io::Result<GBuf>
where
    S: squeue::EntryMarker,
    C: cqueue::EntryMarker,
{
    let read_e = opcode::Read::new(fd, std::ptr::null_mut(), len)
        .offset(0)
        .buf_group(buf_ring.rc.bgid());

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

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x02);

    let result = cqes[0].result();
    if result < 0 {
        // Expect ENOBUFS when the buf_ring is empty.
        return Err(io::Error::from_raw_os_error(-result));
    }

    let result = result as u32;
    assert_eq!(result, len);
    let flags = cqes[0].flags();
    let buf = buf_ring.rc.get_buf(buf_ring.clone(), result, flags)?;

    Ok(buf)
}

// Verify a buf_ring works by seeding one with two buffers, and then play with using it to
// exhaustion and then replenishing.
//
// Two reads should succeed. The next should fail because of buffer exhaustion. Then the buffers
// are released back to the kernel and two more reads should succeed.
fn buf_ring_play<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    _test: &Test,
) -> io::Result<()> {
    // Use with a temporary file that has a line it.
    // Show that the buf ring can be used to fill in data using read operations.
    // In these cases, we keep it simple and the buffers allocated are longer than this text
    // so it is reasonable to expect the entire text be matched by what is put into the buffer
    // by the uring interface.

    let text = b"The quick brown fox jumps over the lazy dog.";
    let len = text.len() as u32;

    let normal_check = |buf: &GBuf, bid: Bid| {
        // Verify the buffer id that was returned to us.
        assert_eq!(bid, buf.bid);

        // Verify the data read into the buffer.
        assert_eq!(buf.as_slice(), text);
    };

    // Build a buf_ring with an arbitrary buffer group id,
    // only two ring entries,
    // and two buffers so the ring starts completely full.
    // Then register it with the uring interface.

    let buf_ring = Builder::new(888)
        .ring_entries(2)
        .buf_cnt(2)
        .buf_len(128)
        .build()?;

    buf_ring.rc.register(ring)?;

    // Create a temporary file with a short sample text we will be reading multiple times.

    let fd = tempfile::tempfile()?;
    let fd = types::Fd(fd.as_raw_fd());
    write_text_to_file(ring, fd, text)?;

    // Use the uring buf_ring feature to have two buffers taken from the buf_ring and read into,
    // from the file, returning the buffer here. The read function is designed to read the same
    // text each time - not normal, but sufficient for this unit test.

    let buf0 = buf_ring_read(ring, &buf_ring, fd, len)?;
    let buf1 = buf_ring_read(ring, &buf_ring, fd, len)?;
    normal_check(&buf0, 0);
    normal_check(&buf1, 1);

    // Expect next read to fail because the ring started with two buffers and those buffer wrappers
    // haven't been dropped yet so the ring should be empty.

    let res2 = buf_ring_read(ring, &buf_ring, fd, len);
    assert_eq!(Some(libc::ENOBUFS), res2.unwrap_err().raw_os_error());

    // Drop in reverse order and see that the two are then used in that reverse order by the uring
    // interface when we perform two more reads.

    std::mem::drop(buf1);
    std::mem::drop(buf0);

    let buf3 = buf_ring_read(ring, &buf_ring, fd, len)?;
    let buf4 = buf_ring_read(ring, &buf_ring, fd, len)?;
    normal_check(&buf3, 1); // bid 1 should come back first.
    normal_check(&buf4, 0); // bid 0 should come back second.

    // Be nice. In this test, the buf_ring is manually unregistered.
    // There is no need to ensure the buffers have been dropped first.
    buf_ring.rc.unregister(ring)?;

    Ok(())
}

pub fn test_register_buf_ring<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> io::Result<()> {
    // register_buf_ring was introduced in kernel 5.19, as was the opcode for UringCmd16.
    // So require the UringCmd16 to avoid running this test on earlier kernels.
    // Another option for the test or an application is to check for one of the 5.19 features
    // although sadly, there is no feature that specifically targets registering a buf_ring.
    // An application can always just try calling the register_buf_ring and determine support by
    // checking the result for success, an older kernel will report an invalid argument.
    require!(
        test;
        test.probe.is_supported(opcode::UringCmd16::CODE);
    );

    println!("test register_buf_ring");

    buf_ring_reg_and_unreg(ring, test)?;

    buf_ring_play(ring, test)?;

    Ok(())
}
