// Create tests for registering buf_rings and for the buf_ring entries.
// The entry point in this file can be found by searching for 'pub'.

use crate::Test;
use io_uring::buf_ring::BufRing;
use io_uring::{cqueue, opcode, squeue, CompletionQueue, IoUring, SubmissionQueue};
use io_uring::{types, Submitter};

use std::cell::UnsafeCell;
use std::fmt;
use std::io;
use std::mem::ManuallyDrop;
use std::os::unix::io::AsRawFd;
use std::rc::Rc;

type Bgid = u16; // Buffer group id
type Bid = u16; // Buffer id

struct InnerBufRing<'a> {
    buf_ring: ManuallyDrop<UnsafeCell<BufRing<'a>>>,
    buf_list: ManuallyDrop<Vec<Vec<u8>>>,
    buf_len: usize,
}

impl<'a> InnerBufRing<'a> {
    fn new(
        submitter: &Submitter<'a>,
        bgid: Bgid,
        ring_entries: u16,
        buf_cnt: u16,
        buf_len: usize,
    ) -> io::Result<InnerBufRing<'a>> {
        // Check that none of the important args are zero and the ring_entries is at least large
        // enough to hold all the buffers and that ring_entries is a power of 2.
        if (buf_cnt == 0)
            || (buf_cnt > ring_entries)
            || (buf_len == 0)
            || ((ring_entries & (ring_entries - 1)) != 0)
        {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }

        let res = submitter.setup_buf_ring(ring_entries, bgid);
        let mut buf_ring = match res {
            Err(e) => match e.raw_os_error() {
                Some(libc::EINVAL) => {
                    // using buf_ring requires kernel 5.19 or greater.
                    return Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!("setup_buf_ring returned {}, most likely indicating this kernel is not 5.19+", e),
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
                                "setup_buf_ring returned `{}`, indicating the attempted buffer group id {} was already registered",
                            e,
                            bgid),
                        ));
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("setup_buf_ring returned `{}` for group id {}", e, bgid),
                    ));
                }
            },
            Ok(buf_ring) => buf_ring,
        };

        // Probably some functional way to do this.
        let mut buf_list: Vec<Vec<u8>> = {
            let mut bp = Vec::with_capacity(buf_cnt as _);
            for _ in 0..buf_cnt {
                bp.push(vec![0; buf_len]);
            }
            bp
        };
        unsafe {
            buf_ring.push_multiple(buf_list.iter_mut().enumerate().map(|(i, b)| {
                (
                    i as u16,
                    std::slice::from_raw_parts_mut(b.as_mut_ptr().cast(), b.capacity()),
                )
            }));
        }

        let buf_ring = InnerBufRing {
            buf_ring: ManuallyDrop::new(UnsafeCell::new(buf_ring)),
            buf_list: ManuallyDrop::new(buf_list),
            buf_len,
        };

        Ok(buf_ring)
    }

    fn unregister(mut self) -> io::Result<()> {
        unsafe {
            ManuallyDrop::into_inner(std::ptr::read(&self.buf_ring))
                .into_inner()
                .unregister()?;
            ManuallyDrop::drop(&mut self.buf_list);
        }
        std::mem::forget(self);
        Ok(())
    }
    // Returns the buffer group id.
    fn bgid(&self) -> Bgid {
        unsafe { &*self.buf_ring.get() }.bgid()
    }

    // Returns the buffer the uring interface picked from the buf_ring for the completion result
    // represented by the res and flags.
    fn get_buf(
        &self,
        buf_ring: FixedSizeBufRing<'a>,
        res: u32,
        flags: u32,
    ) -> io::Result<GBuf<'a>> {
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
    }

    fn buf_capacity(&self) -> usize {
        self.buf_len as _
    }

    fn stable_ptr(&self, bid: Bid) -> *const u8 {
        self.buf_list[bid as usize].as_ptr()
    }

    // Push the `bid` buffer to the buf_ring tail.
    // This test version does not safeguard against a duplicate
    // `bid` being pushed.
    fn buf_ring_push(&self, bid: Bid) {
        assert!((bid as usize) < self.buf_list.len());

        let buf = &self.buf_list[bid as usize];
        unsafe {
            (*self.buf_ring.get()).push(
                bid,
                std::slice::from_raw_parts_mut(buf.as_ptr().cast_mut().cast(), buf.capacity()),
            );
        }
    }
}

impl Drop for InnerBufRing<'_> {
    fn drop(&mut self) {
        unsafe {
            std::ptr::read(self).unregister().ok();
        }
    }
}

#[derive(Clone)]
struct FixedSizeBufRing<'a> {
    // The BufRing is reference counted because each buffer handed out has a reference back to its
    // buffer group, or in this case, to its buffer ring.
    rc: Rc<InnerBufRing<'a>>,
}

impl<'a> FixedSizeBufRing<'a> {
    fn new(buf_ring: InnerBufRing<'a>) -> Self {
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
    fn build<'a>(&self, submitter: &Submitter<'a>) -> io::Result<FixedSizeBufRing<'a>> {
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

        let inner = InnerBufRing::new(submitter, b.bgid, b.ring_entries, b.buf_cnt, b.buf_len)?;
        Ok(FixedSizeBufRing::new(inner))
    }
}

// This tracks a buffer that has been filled in by the kernel, having gotten the memory
// from a buffer ring, and returned to userland via a cqe entry.
struct GBuf<'a> {
    bufgroup: FixedSizeBufRing<'a>,
    len: usize,
    bid: Bid,
}

impl fmt::Debug for GBuf<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GBuf")
            .field("bgid", &self.bufgroup.rc.bgid())
            .field("bid", &self.bid)
            .field("len", &self.len)
            .field("cap", &self.bufgroup.rc.buf_capacity())
            .finish()
    }
}

impl<'a> GBuf<'a> {
    fn new(bufgroup: FixedSizeBufRing<'a>, bid: Bid, len: usize) -> Self {
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

impl Drop for GBuf<'_> {
    fn drop(&mut self) {
        // Add the buffer back to the bufgroup, for the kernel to reuse.
        unsafe { self.bufgroup.rc.dropping_bid(self.bid) };
    }
}

// Begin of test functions.

// Verify register and unregister of a buf_ring.
fn buf_ring_reg_and_unreg(submitter: &Submitter, _test: &Test) -> io::Result<()> {
    // Create a BufRing
    // Register it
    // Unregister it
    //
    // Register it
    // Try to register it again
    // Unregister it
    // Try to unnregister it again
    // Drop it

    let buf_ring = Builder::new(777)
        .ring_entries(16)
        .buf_len(4096)
        .build(submitter)?;

    Rc::try_unwrap(buf_ring.rc)
        .unwrap_or_else(|_| unreachable!())
        .unregister()?;

    Ok(())
}

// Write sample to file descriptor.
fn write_text_to_file<S, C>(
    submitter: &Submitter,
    sq: &mut SubmissionQueue<S>,
    cq: &mut CompletionQueue<C>,
    fd: types::Fd,
    text: &[u8],
) -> io::Result<()>
where
    S: squeue::EntryMarker,
    C: cqueue::EntryMarker,
{
    let write_e = opcode::Write::new(fd, text.as_ptr(), text.len() as _);

    unsafe {
        let write_e = write_e
            .build()
            .user_data(0x01)
            .flags(squeue::Flags::IO_LINK)
            .into();
        sq.push(&write_e).expect("queue is full");
        sq.sync();
    }
    assert_eq!(submitter.submit_and_wait(1)?, 1);

    cq.sync();
    let cqes: Vec<cqueue::Entry> = cq.map(Into::into).collect();
    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x01);
    assert_eq!(cqes[0].result(), text.len() as i32);
    Ok(())
}

// Read from file descriptor, returning a buffer from the buf_ring.
fn buf_ring_read<'a, S, C>(
    submitter: &Submitter,
    sq: &mut SubmissionQueue<S>,
    cq: &mut CompletionQueue<C>,
    buf_ring: &FixedSizeBufRing<'a>,
    fd: types::Fd,
    len: u32,
) -> io::Result<GBuf<'a>>
where
    S: squeue::EntryMarker,
    C: cqueue::EntryMarker,
{
    let read_e = opcode::Read::new(fd, std::ptr::null_mut(), len)
        .offset(0)
        .buf_group(buf_ring.rc.bgid());

    unsafe {
        sq.push(
            &read_e
                .build()
                .user_data(0x02)
                .flags(squeue::Flags::BUFFER_SELECT)
                .into(),
        )
        .expect("queue is full");
        sq.sync();
    }
    assert_eq!(submitter.submit_and_wait(1)?, 1);

    cq.sync();
    let cqes: Vec<cqueue::Entry> = cq.map(Into::into).collect();

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

    let (submitter, mut sq, mut cq) = ring.split();

    let buf_ring = Builder::new(888)
        .ring_entries(2)
        .buf_cnt(2)
        .buf_len(128)
        .build(&submitter)?;

    // Create a temporary file with a short sample text we will be reading multiple times.

    let fd = tempfile::tempfile()?;
    let fd = types::Fd(fd.as_raw_fd());
    write_text_to_file(&submitter, &mut sq, &mut cq, fd, text)?;

    // Use the uring buf_ring feature to have two buffers taken from the buf_ring and read into,
    // from the file, returning the buffer here. The read function is designed to read the same
    // text each time - not normal, but sufficient for this unit test.

    let buf0 = buf_ring_read(&submitter, &mut sq, &mut cq, &buf_ring, fd, len)?;
    let buf1 = buf_ring_read(&submitter, &mut sq, &mut cq, &buf_ring, fd, len)?;
    normal_check(&buf0, 0);
    normal_check(&buf1, 1);

    // Expect next read to fail because the ring started with two buffers and those buffer wrappers
    // haven't been dropped yet so the ring should be empty.

    let res2 = buf_ring_read(&submitter, &mut sq, &mut cq, &buf_ring, fd, len);
    assert_eq!(Some(libc::ENOBUFS), res2.unwrap_err().raw_os_error());

    // Drop in reverse order and see that the two are then used in that reverse order by the uring
    // interface when we perform two more reads.

    std::mem::drop(buf1);
    std::mem::drop(buf0);

    let buf3 = buf_ring_read(&submitter, &mut sq, &mut cq, &buf_ring, fd, len)?;
    let buf4 = buf_ring_read(&submitter, &mut sq, &mut cq, &buf_ring, fd, len)?;
    normal_check(&buf3, 1); // bid 1 should come back first.
    normal_check(&buf4, 0); // bid 0 should come back second.

    std::mem::drop(buf3);
    std::mem::drop(buf4);

    // Now we loop u16::MAX times to ensure proper behavior when the tail
    // overflows the bounds of a u16.
    for _ in 0..=u16::MAX {
        let _ = buf_ring_read(&submitter, &mut sq, &mut cq, &buf_ring, fd, len)?;
    }

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

    buf_ring_reg_and_unreg(&ring.submitter(), test)?;

    buf_ring_play(ring, test)?;

    Ok(())
}
