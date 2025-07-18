use crate::Test;
use io_uring::{
    cqueue::{self, EntryMarker},
    opcode::{ReadFixed, WriteFixed},
    squeue,
    types::Fd,
    IoUring,
};
use libc::iovec;
use std::{
    fs::File,
    io::{self, IoSliceMut},
    io::{Error, Write},
    ops::DerefMut,
    os::fd::AsRawFd,
    os::fd::FromRawFd,
};

use io_uring::{opcode, types};
use libc::EFAULT;

pub fn test_register_buffers<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    _test_register_buffers(
        ring,
        test,
        "register_buffers",
        None,
        |ring, iovecs, _| unsafe { ring.submitter().register_buffers(&iovecs) },
    )?;
    _test_register_buffers(
        ring,
        test,
        "register_buffers2",
        ring.params().is_feature_resource_tagging(),
        |ring, iovecs, tags| unsafe { ring.submitter().register_buffers2(iovecs, tags) },
    )?;
    _test_register_buffers(
        ring,
        test,
        "register_buffers_sparse",
        ring.params().is_feature_resource_tagging(),
        |ring, iovecs, _| {
            let submitter = ring.submitter();
            submitter.register_buffers_sparse(iovecs.len() as _)?;
            unsafe { submitter.register_buffers_update(0, iovecs, None) }
        },
    )?;

    return Ok(());
}

fn _test_register_buffers<
    S: squeue::EntryMarker,
    C: cqueue::EntryMarker,
    F: FnMut(&mut IoUring<S, C>, &[iovec], &[u64]) -> io::Result<()>,
    P: Into<Option<bool>>,
>(
    ring: &mut IoUring<S, C>,
    test: &Test,
    name: &str,
    probe: P,
    mut register: F,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(WriteFixed::CODE);
        test.probe.is_supported(ReadFixed::CODE);
        probe.into().unwrap_or(true);
    );

    println!("test {name}");

    const BUF_SIZE: usize = 1 << 12; // Page size
    const BUFFERS: usize = 8; // The maximum number of squeue entries (in main.rs)

    let file = tempfile::tempfile()?;
    let fd = Fd(file.as_raw_fd());

    // Create the buffers
    let mut bufs = (0..BUFFERS)
        .map(|i| vec![b'A' + i as u8; BUF_SIZE])
        .collect::<Vec<_>>();

    // Create the slices that point to the buffers
    let mut slices: Vec<IoSliceMut<'_>> = bufs
        .iter_mut()
        .map(|buf| IoSliceMut::new(buf.deref_mut()))
        .collect();

    // Check that the data is correct
    assert!(slices
        .iter()
        .enumerate()
        .all(|(i, s)| s.iter().all(|&x| x == (b'A' + i as u8))));

    // Now actually set up and register the buffers

    // Safety: `IoSliceMut` is ABI compatible with the `iovec` type on Unix platforms, so it is safe
    // to cast these as `slices` is valid for this entire function
    let iovecs: &[iovec] =
        unsafe { std::slice::from_raw_parts(slices.as_ptr().cast(), slices.len()) };
    let tags = vec![0; slices.len()];

    // Safety: Since `iovecs` is derived from valid `IoSliceMut`s, this upholds the safety contract
    // of `register_buffers` that the buffers are valid until the buffers are unregistered
    register(ring, iovecs, &tags).unwrap();

    // Prepare writing the buffers out to the file
    (0..BUFFERS).for_each(|index| {
        let buf_ptr = slices[index].as_ptr();

        let write_entry = WriteFixed::new(fd, buf_ptr.cast(), BUF_SIZE as u32, index as u16)
            .offset((index * BUF_SIZE) as u64)
            .build()
            .user_data(index as u64);

        let mut submission_queue = ring.submission();
        // Safety: We have guaranteed that the buffers in `slices` are all valid for the entire
        // duration of this function
        unsafe {
            submission_queue.push(write_entry.into()).unwrap();
        }
    });

    // Submit and wait for the kernel to complete all of the operations
    assert_eq!(ring.submit_and_wait(BUFFERS)?, BUFFERS);

    // The file should have saved all of the data now
    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
    cqes.iter().enumerate().for_each(|(index, ce)| {
        assert!(ce.user_data() < BUFFERS as u64);
        assert_eq!(
            ce.result(),
            BUF_SIZE as i32,
            "WriteFixed operation {} failed",
            index
        );
    });

    // Zero out all buffers in memory
    (0..BUFFERS).for_each(|index| {
        slices[index].fill(0);
        assert!(slices[index].iter().all(|&x| x == 0));
    });

    // Prepare reading the data back into the buffers from the file
    (0..BUFFERS).for_each(|index| {
        let buf_ptr = slices[index].as_mut_ptr();

        let read_entry = ReadFixed::new(fd, buf_ptr.cast(), BUF_SIZE as u32, index as u16)
            .offset((index * BUF_SIZE) as u64)
            .build()
            .user_data(index as u64);

        let mut submission_queue = ring.submission();
        // Safety: We have guaranteed that the buffers in `slices` are all valid for the entire
        // duration of this function
        unsafe {
            submission_queue.push(read_entry.into()).unwrap();
        }
    });

    // Submit and wait for the kernel to complete all of the operations
    assert_eq!(ring.submit_and_wait(BUFFERS)?, BUFFERS);

    // The data should be back in the buffers
    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
    cqes.iter().enumerate().for_each(|(index, ce)| {
        assert!(ce.user_data() < BUFFERS as u64);
        assert_eq!(
            ce.result(),
            BUF_SIZE as i32,
            "ReadFixed operation {} failed",
            index
        );
    });

    // Check that the data has been restored
    assert!((0..BUFFERS).all(|index| { slices[index].iter().all(|&x| x == (b'A' + index as u8)) }));

    // Unregister the buffers
    let submitter = ring.submitter();
    submitter.unregister_buffers()?;

    Ok(())
}

const BUFFER_TAG: u64 = 0xbadcafe;
const TIMEOUT_TAG: u64 = 0xbadf00d;

pub fn test_register_buffers_update<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::ReadFixed::CODE);
        ring.params().is_feature_resource_tagging();
    );

    println!("test register_buffers_update");

    let (read, mut write) = create_pipe()?;
    let mut buf = [0xde, 0xed, 0xbe, 0xef];
    let mut tags = [BUFFER_TAG];
    let iovecs = [libc::iovec {
        iov_len: buf.len(),
        iov_base: buf.as_mut_ptr() as _,
    }];
    let timeout = types::Timespec::new().nsec(50 * 1_000_000);
    let timeout = opcode::Timeout::new(&timeout as _)
        .build()
        .user_data(TIMEOUT_TAG);
        // .into();
    let read_sqe = opcode::ReadFixed::new(
        types::Fd(read.as_raw_fd()),
        buf.as_mut_ptr(),
        buf.len() as _,
        5,
    )
    .build()
    .user_data(42);

    // Register a buffer table and then immediately unregister it
    ring.submitter().register_buffers_sparse(1)?;
    ring.submitter().unregister_buffers()?;

    // Push a timeout of 50ms
    unsafe { ring.submission().push(timeout.clone().into()).unwrap() };

    // We should not receive any other entries than the timeout
    check_only_timeout(ring)?;

    // Register a sparse buffer table of 10 elements
    ring.submitter().register_buffers_sparse(10)?;

    // Try read the pipe using a sparse buffer
    let cqe = {
        write.write("yo".as_bytes())?;

        unsafe { ring.submission().push(read_sqe.clone().into()).unwrap() };

        ring.submit_and_wait(1)?;

        ring.completion().next().unwrap().into()
    };

    // We should get the correct user_data
    if cqe.user_data() != 42 {
        return Err(anyhow::anyhow!("unexpected completion queue entry"));
    }

    // EFAULT is to be expected with incorrect fixed buffers
    if cqe.result() != -EFAULT {
        return Err(anyhow::anyhow!("unexpected read result: {}", cqe.result()));
    }

    // Register a buffer at the index 5
    unsafe {
        ring.submitter()
            .register_buffers_update(5, &iovecs, Some(&tags))?;

        // Push a timeout of 50ms
        ring.submission().push(timeout.into()).unwrap();
    }

    // We should not receive any other entries than the timeout
    check_only_timeout(ring)?;

    // Register a buffer at the same index 5, but this time with an empty tag.
    let cqe = {
        tags[0] = 0;

        unsafe {
            ring.submitter()
                .register_buffers_update(5, &iovecs, Some(&tags))?;
        }
        ring.submit_and_wait(1)?;

        ring.completion().next().unwrap().into()
    };

    // We should get a cqe with the first tag because we registered a
    // new buffer at an index where a buffer was already registered.
    if cqe.user_data() != BUFFER_TAG {
        return Err(anyhow::anyhow!(
            "expected completion queue to contain a buffer unregistered event"
        ));
    }

    // Try reading now that the buffer is registered at index 5
    let cqe = {
        unsafe {
            ring.submission().push(read_sqe.into()).unwrap();
        }

        ring.submit_and_wait(1)?;

        ring.completion().next().unwrap().into()
    };

    // We should get the correct user_data
    if cqe.user_data() != 42 {
        return Err(anyhow::anyhow!("unexpected completion queue entry"));
    }

    // We should read exactly two bytes
    if cqe.result() != 2 {
        return Err(anyhow::anyhow!("unexpected read result: {}", cqe.result()));
    }

    // The first two bytes of `buf` should be "yo"
    if &buf[0..2] != "yo".as_bytes() {
        return Err(anyhow::anyhow!("unexpected read buffer data: {:x?}", &buf));
    }

    return Ok(());
}

/// Create a pipe and return both ends as RAII `File` handles
fn create_pipe() -> io::Result<(File, File)> {
    let mut fds = [-1, -1];

    unsafe {
        if libc::pipe(fds.as_mut_ptr()) == -1 {
            Err(Error::last_os_error())
        } else {
            Ok((File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])))
        }
    }
}

/// Submit sqes and asserts the only cqe is a timeout entry
fn check_only_timeout<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
) -> Result<(), anyhow::Error> {
    ring.submit_and_wait(1)?;

    if Into::<cqueue::Entry>::into(ring.completion().next().unwrap()).user_data() == TIMEOUT_TAG {
        // There should not be any more entries in the queue
        if ring.completion().next().is_none() {
            return Ok(());
        }
    }

    Err(anyhow::anyhow!("unexpected completion queue entry"))
}
