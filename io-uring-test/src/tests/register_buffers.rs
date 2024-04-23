use crate::Test;
use io_uring::{
    cqueue,
    opcode::{ReadFixed, WriteFixed},
    squeue,
    types::Fd,
    IoUring,
};
use libc::iovec;
use std::{
    io::{self, IoSliceMut},
    os::fd::AsRawFd,
};

/// Creates shared buffers to be registered into the [`IoUring`] instance, and then tests that they
/// can actually be used through the [`WriteFixed`] and [`ReadFixed`] opcodes.
pub fn test_register_buffers<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> io::Result<()> {
    require!(
        test;
        test.probe.is_supported(WriteFixed::CODE);
        test.probe.is_supported(ReadFixed::CODE);
    );

    println!("test register_buffers");

    const BUF_SIZE: usize = 1 << 12; // Page size
    const BUFFERS: usize = 8; // The maximum number of squeue entries (in main.rs)

    let file = tempfile::tempfile()?;
    let fd = Fd(file.as_raw_fd());

    // Create the buffers
    let slices = (0..BUFFERS)
        .map(|i| {
            let v = vec![b'A' + i as u8; BUF_SIZE];
            let slice = v.leak();
            IoSliceMut::new(slice)
        })
        .collect::<Vec<_>>();

    // Check that the data is correct
    slices
        .iter()
        .enumerate()
        .for_each(|(i, s)| assert!(s.iter().all(|&x| x == (b'A' + i as u8))));

    // Register the buffers
    let iovecs: &[iovec] =
        unsafe { std::slice::from_raw_parts(slices.as_ptr().cast(), slices.len()) };
    let submitter = ring.submitter();
    unsafe { submitter.register_buffers(iovecs)? };

    // Prepare writing the buffers out to the file
    (0..BUFFERS).for_each(|index| {
        let buf_ptr = iovecs[index].iov_base;

        let write_entry = WriteFixed::new(fd, buf_ptr.cast(), BUF_SIZE as u32, index as u16)
            .offset((index * BUF_SIZE) as u64)
            .build()
            .user_data(index as u64);

        let mut submission_queue = ring.submission();
        unsafe {
            submission_queue.push(&write_entry.into()).unwrap();
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
        let iov = iovecs[index];
        let slice: &mut [u8] =
            unsafe { std::slice::from_raw_parts_mut(iov.iov_base.cast(), iov.iov_len) };
        slice.fill(0);
        assert!(slice.iter().all(|&x| x == 0));
    });

    // Prepare reading the data back into the buffers from the file
    (0..BUFFERS).for_each(|index| {
        let buf_ptr = iovecs[index].iov_base;

        let read_entry = ReadFixed::new(fd, buf_ptr.cast(), BUF_SIZE as u32, index as u16)
            .offset((index * BUF_SIZE) as u64)
            .build()
            .user_data(index as u64);

        let mut submission_queue = ring.submission();
        unsafe {
            submission_queue.push(&read_entry.into()).unwrap();
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
    (0..BUFFERS).for_each(|index| {
        let iov = iovecs[index];
        let slice: &mut [u8] =
            unsafe { std::slice::from_raw_parts_mut(iov.iov_base.cast(), iov.iov_len) };
        assert!(slice.iter().all(|&x| x == (b'A' + index as u8)));
    });

    // Unregister the buffers
    let submitter = ring.submitter();
    submitter.unregister_buffers()?;

    Ok(())
}
