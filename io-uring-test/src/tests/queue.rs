use crate::Test;
use io_uring::{cqueue, opcode, squeue, types, IoUring};
use std::time::{Duration, Instant};

pub fn test_nop<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require! {
        test;
    }

    println!("test nop");

    let nop_e = opcode::Nop::new().build().user_data(0x42).into();

    unsafe {
        let mut queue = ring.submission();
        queue.push(&nop_e).expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x42);
    assert_eq!(cqes[0].result(), 0);

    Ok(())
}

pub fn test_setup_no_sqarray<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    _ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require! {
        test;
    }

    println!("test setup_no_sqarray");

    let mut ring = match IoUring::<S, C>::builder().setup_no_sqarray().build(8) {
        Ok(ring) => ring,
        Err(err) => match err.raw_os_error() {
            Some(libc::EINVAL) | Some(libc::EOPNOTSUPP) => {
                println!("IORING_SETUP_NO_SQARRAY is not supported by the kernel, skip");
                return Ok(());
            }
            _ => return Err(err.into()),
        },
    };

    let nop_e = opcode::Nop::new().build().user_data(0x4242).into();

    unsafe {
        ring.submission().push(&nop_e).expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x4242);
    assert_eq!(cqes[0].result(), 0);

    Ok(())
}

pub fn test_setup_no_mmap<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    _ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require! {
        test;
    }

    println!("test setup_no_mmap");

    // 6.5 and 6.6 reject regions larger than one page with EFAULT, 6.7 to 6.9 accept them only
    // if physically contiguous (else EINVAL), and 6.10 vmaps any memory.
    let vmaps_user_pages = kernel_version() >= (6, 10);
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;

    // 1024 entries need several pages per region, so they are not physically contiguous.
    let entries = 1024;
    let mut cqsize = IoUring::<S, C>::builder();
    cqsize.setup_cqsize(4 * entries);
    for builder in [IoUring::<S, C>::builder(), cqsize] {
        let (rings_len, sqes_len) = builder.no_mmap_sizes(entries);
        let memory = ZeroedMemory::new(rings_len + sqes_len);
        let (rings, sqes) = memory.split(rings_len);

        let mut ring = match unsafe {
            builder.build_no_mmap(entries, rings, rings_len, sqes, sqes_len)
        } {
            Ok(ring) => ring,
            Err(err)
                if !vmaps_user_pages
                    && matches!(err.raw_os_error(), Some(libc::EINVAL | libc::EFAULT)) =>
            {
                println!("IORING_SETUP_NO_MMAP with discontiguous pages is not supported by the kernel, skip");
                return Ok(());
            }
            Err(err) => return Err(err.into()),
        };

        for i in 0..4u64 {
            let nop_e = opcode::Nop::new().build().user_data(0x4242 + i).into();
            unsafe {
                ring.submission().push(&nop_e).expect("queue is full");
            }
        }

        ring.submit_and_wait(4)?;

        // The kernel wrote the completions into the caller's memory, not a ring of its own.
        let words =
            unsafe { std::slice::from_raw_parts(rings.as_ptr().cast::<u64>(), rings_len / 8) };
        for i in 0..4u64 {
            assert!(words.contains(&(0x4242 + i)));
        }
        assert!(ring.params().is_setup_no_mmap());

        let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
        assert_eq!(cqes.len(), 4);
        for (i, cqe) in cqes.iter().enumerate() {
            assert_eq!(cqe.user_data(), 0x4242 + i as u64);
            assert_eq!(cqe.result(), 0);
        }

        drop(ring);
        assert!(
            memory.is_mapped(),
            "dropping the ring unmapped the caller's memory"
        );

        // Regions one page smaller than no_mmap_sizes, and a single page, are refused rather
        // than overrun.
        for (rings_len, sqes_len) in [
            (rings_len - page, sqes_len),
            (rings_len, sqes_len - page),
            (page, sqes_len),
        ] {
            let memory = ZeroedMemory::new(rings_len + sqes_len);
            let (rings, sqes) = memory.split(rings_len);
            let err = unsafe { builder.build_no_mmap(entries, rings, rings_len, sqes, sqes_len) }
                .err()
                .expect("an undersized region must be refused");
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        }
    }

    Ok(())
}

fn kernel_version() -> (u32, u32) {
    let mut uts = unsafe { std::mem::zeroed::<libc::utsname>() };
    assert_eq!(unsafe { libc::uname(&mut uts) }, 0);
    let release = unsafe { std::ffi::CStr::from_ptr(uts.release.as_ptr()) }.to_string_lossy();
    let mut numbers = release
        .split(|c: char| !c.is_ascii_digit())
        .map(|n| n.parse().unwrap_or(0));
    (numbers.next().unwrap_or(0), numbers.next().unwrap_or(0))
}

struct ZeroedMemory {
    addr: std::ptr::NonNull<u8>,
    len: usize,
}

impl ZeroedMemory {
    fn new(len: usize) -> Self {
        let addr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        assert_ne!(addr, libc::MAP_FAILED);
        let addr = std::ptr::NonNull::new(addr.cast()).unwrap();
        ZeroedMemory { addr, len }
    }

    fn split(&self, at: usize) -> (std::ptr::NonNull<u8>, std::ptr::NonNull<u8>) {
        (self.addr, unsafe { self.addr.add(at) })
    }

    fn is_mapped(&self) -> bool {
        unsafe { libc::msync(self.addr.as_ptr().cast(), self.len, libc::MS_ASYNC) == 0 }
    }
}

impl Drop for ZeroedMemory {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.addr.as_ptr().cast(), self.len) };
    }
}

pub fn test_defer_taskrun<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    test: &Test,
) -> anyhow::Result<()> {
    require! {
        test;
    }

    println!("test defer_taskrun");

    for with_args in [false, true] {
        let mut builder = IoUring::<S, C>::builder();
        builder
            .setup_single_issuer()
            .setup_defer_taskrun()
            .setup_taskrun_flag();
        let mut ring = match builder.build(8) {
            Ok(ring) => ring,
            Err(err) => match err.raw_os_error() {
                Some(libc::EINVAL) | Some(libc::EOPNOTSUPP) => {
                    println!("IORING_SETUP_DEFER_TASKRUN is not supported by the kernel, skip");
                    return Ok(());
                }
                _ => return Err(err.into()),
            },
        };

        let timeout = types::Timespec::new().nsec(1_000_000);
        let entry = opcode::Timeout::new(&timeout)
            .build()
            .user_data(0x42)
            .into();
        unsafe {
            ring.submission().push(&entry).expect("queue is full");
        }
        ring.submitter().submit()?;

        let deadline = Instant::now() + Duration::from_secs(5);
        while !ring.submission().taskrun() {
            assert!(Instant::now() < deadline, "IORING_SQ_TASKRUN was not set");
            std::thread::yield_now();
        }

        if with_args {
            let zero = types::Timespec::new();
            let args = types::SubmitArgs::new().timespec(&zero);
            ring.submitter().submit_with_args(0, &args)?;
        } else {
            ring.submitter().submit_and_wait(0)?;
        }

        assert!(!ring.submission().taskrun());
        let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
        assert_eq!(cqes.len(), 1);
        assert_eq!(cqes[0].user_data(), 0x42);
        assert_eq!(cqes[0].result(), -libc::ETIME);
    }

    Ok(())
}

pub fn test_batch<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    use std::mem::MaybeUninit;

    require! {
        test;
    }

    println!("test batch");

    assert!(ring.completion().is_empty());

    unsafe {
        let sqes = vec![opcode::Nop::new().build().user_data(0x09).into(); 5];
        let mut sq = ring.submission();

        assert_eq!(sq.capacity(), 8);

        sq.push_multiple(&sqes).unwrap();

        assert_eq!(sq.len(), 5);

        let ret = sq.push_multiple(&sqes);
        assert!(ret.is_err());

        assert_eq!(sq.len(), 5);

        sq.push_multiple(&sqes[..3]).unwrap();
    }

    ring.submit_and_wait(8)?;

    let mut cqes = (0..10).map(|_| MaybeUninit::uninit()).collect::<Vec<_>>();

    let cqes = ring.completion().fill(&mut cqes);

    assert_eq!(cqes.len(), 8);

    for entry in cqes {
        let entry: cqueue::Entry = entry.clone().into();
        assert_eq!(entry.user_data(), 0x09);
    }

    Ok(())
}

pub fn test_queue_split<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require! {
        test;
    }

    println!("test queue_split");

    let (submitter, mut sq, mut cq) = ring.split();

    assert!(sq.is_empty());

    for _ in 0..sq.capacity() {
        unsafe {
            sq.push(&opcode::Nop::new().build().into())
                .expect("queue is full");
        }
    }

    assert!(sq.is_full());

    sq.sync();

    assert_eq!(submitter.submit()?, sq.capacity());
    assert!(sq.is_full());
    sq.sync();
    assert!(sq.is_empty());

    assert!(cq.is_empty());
    cq.sync();
    assert_eq!(cq.len(), sq.capacity());
    assert_eq!(cq.by_ref().count(), sq.capacity());

    cq.sync();

    Ok(())
}

pub fn test_debug_print<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require! {
        test;
    }

    println!("test debug_print");

    let mut sq = ring.submission();
    let num_to_sub = sq.capacity();
    for _ in 0..num_to_sub {
        unsafe {
            sq.push(&opcode::Nop::new().build().user_data(0x42).into())
                .expect("queue is full");
        }
    }
    println!("Full: {:?}", sq);
    drop(sq);

    ring.submit_and_wait(num_to_sub)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), num_to_sub);
    for cqe in cqes {
        assert_eq!(cqe.user_data(), 0x42);
        assert_eq!(cqe.result(), 0);
    }
    println!("Empty: {:?}", ring.submission());

    Ok(())
}

pub fn test_msg_ring_data<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    use std::os::unix::io::AsRawFd;

    require!(
        test;
        test.probe.is_supported(opcode::MsgRingData::CODE);
    );

    println!("test msg_ring_data");

    // Create a new destination ring and send a data message to it through
    // the existing test/source ring. This will generate two completion events,
    // one on each ring.
    let mut dest_ring = IoUring::new(1)?;
    let fd = types::Fd(dest_ring.as_raw_fd());
    let result = 82; // b'R'
    let user_data = 85; // b'U'
    unsafe {
        ring.submission()
            .push(
                &opcode::MsgRingData::new(fd, result, user_data, None)
                    .build()
                    .into(),
            )
            .expect("queue is full");
    }
    ring.submit_and_wait(1)?;

    let source_cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
    assert_eq!(source_cqes.len(), 1);
    assert_eq!(source_cqes[0].user_data(), 0);
    assert_eq!(source_cqes[0].result(), 0);
    assert_eq!(source_cqes[0].flags(), 0);

    let dest_cqes: Vec<cqueue::Entry> = dest_ring.completion().map(Into::into).collect();
    assert_eq!(dest_cqes.len(), 1);
    assert_eq!(dest_cqes[0].user_data(), user_data);
    assert_eq!(dest_cqes[0].result(), result);
    assert_eq!(dest_cqes[0].flags(), 0);

    Ok(())
}

pub fn test_msg_ring_send_fd<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    use std::os::unix::io::AsRawFd;

    // This test requires both `IORING_OP_MSG_RING` opcode and `IORING_MSG_SEND_FD` flag.
    // We directly probe for the opcode, but the flag only appeared later
    // (in kernel 6.0) and cannot be probed at runtime.
    // Thus, we additionaly check for the `IORING_OP_SEND_ZC` opcode (>= 6.0)
    // as a proxy instead.
    require!(
        test;
        test.probe.is_supported(opcode::MsgRingData::CODE);
        test.probe.is_supported(opcode::SendZc::CODE);
    );

    println!("test msg_ring_send_fd");

    // Ensure there are no fixed files on the test ring, so that slots
    // allocation will start at 0. Then create a tempfile and register it
    // (at slot 0).
    {
        let _ = ring.submitter().unregister_files();
        let tmp1 = tempfile::tempfile()?;
        ring.submitter()
            .register_files(&[tmp1.as_raw_fd()])
            .unwrap();
    }

    // Create a new destination ring.
    // FD sending only works if the receiving ring already has a slot
    // registered for it. Thus we register some sparse files (slots 0 and 1).
    let mut temp_ring = IoUring::new(8)?;
    temp_ring.submitter().register_files_sparse(2).unwrap();

    // Send the fixed file from the source ring (on slot 0)
    // to the temp ring (to slot 1).
    // This will generate two completion events, one on each ring.
    unsafe {
        let fd = types::Fd(temp_ring.as_raw_fd());
        let dest_slot = types::DestinationSlot::try_from_slot_target(1).unwrap();
        ring.submission()
            .push(
                &opcode::MsgRingSendFd::new(fd, types::Fixed(0), dest_slot, 22)
                    .build()
                    .into(),
            )
            .expect("queue is full");
    }
    ring.submit_and_wait(1)?;
    {
        let source_cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
        assert_eq!(source_cqes.len(), 1);
        assert_eq!(source_cqes[0].user_data(), 0);
        assert_eq!(source_cqes[0].result(), 0);
        assert_eq!(source_cqes[0].flags(), 0);

        let dest_cqes: Vec<cqueue::Entry> = temp_ring.completion().map(Into::into).collect();
        assert_eq!(dest_cqes.len(), 1);
        assert_eq!(dest_cqes[0].user_data(), 22);
        assert_eq!(dest_cqes[0].result(), 0);
        assert_eq!(dest_cqes[0].flags(), 0);
    }

    // Unregister the fixed files from the source ring, then reserve some empty slots
    // (0 through 3).
    ring.submitter().unregister_files().unwrap();
    ring.submitter().register_files_sparse(4).unwrap();

    // Send back the fixed file from the temp ring (on slot 1)
    // to the temp ring (to slot 2).
    // This will again generate two completion events, one on each ring.
    unsafe {
        let fd = types::Fd(ring.as_raw_fd());
        let dest_slot = types::DestinationSlot::try_from_slot_target(2).unwrap();
        temp_ring
            .submission()
            .push(&opcode::MsgRingSendFd::new(fd, types::Fixed(1), dest_slot, 44).build())
            .expect("queue is full");
    }
    temp_ring.submit_and_wait(1)?;
    {
        let source_cqes: Vec<cqueue::Entry> = temp_ring.completion().map(Into::into).collect();
        assert_eq!(source_cqes.len(), 1);
        assert_eq!(source_cqes[0].user_data(), 0);
        assert_eq!(source_cqes[0].result(), 0);
        assert_eq!(source_cqes[0].flags(), 0);

        let dest_cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
        assert_eq!(dest_cqes.len(), 1);
        assert_eq!(dest_cqes[0].user_data(), 44);
        assert_eq!(dest_cqes[0].result(), 0);
        assert_eq!(dest_cqes[0].flags(), 0);
    }

    // Unregister the fixed files from both rings, then repeat again to
    // ensure that all previous files have been unregistered (on empty rings
    // the operation will fail with `-ENXIO`).
    ring.submitter().unregister_files().unwrap();
    temp_ring.submitter().unregister_files().unwrap();
    ring.submitter().unregister_files().unwrap_err();
    temp_ring.submitter().unregister_files().unwrap_err();

    Ok(())
}

pub fn test_completion_status<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require! {
        test;
    }

    println!("test completion_status");

    // SAFETY: the status is dropped well before `ring` is.
    let status = unsafe { ring.completion().status() };

    assert!(status.is_empty());

    let nop_e = opcode::Nop::new().build().user_data(0x43).into();
    unsafe {
        ring.submission().push(&nop_e).expect("queue is full");
    }
    ring.submit_and_wait(1)?;

    // The status observes the kernel's tail directly, so it reports the
    // completion without the queue having been synchronized.
    assert!(!status.is_empty());

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x43);

    // ... and consuming it moves the head back level with the tail.
    assert!(status.is_empty());

    Ok(())
}
