use crate::Test;
use io_uring::{cqueue, opcode, squeue, IoUring};
use std::{
    io::{PipeReader, PipeWriter, Read, Write},
    os::fd::FromRawFd,
};

pub fn test_pipe<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Pipe::CODE);
    );

    println!("test pipe");

    const REQ_TYPE_PIPE: u64 = 1;
    const DATA: &[u8] = b"foo";

    // Setup placeholder fds to be assigned on op completion.
    let mut fds = [-1, -1];

    // Submit pipe op.
    let sqe = opcode::Pipe::new(fds.as_mut_ptr())
        .build()
        .user_data(REQ_TYPE_PIPE)
        .into();
    unsafe { ring.submission().push(&sqe) }?;
    ring.submit_and_wait(1)?;

    // Process pipe op.
    let cqe = ring
        .completion()
        .map(Into::<cqueue::Entry>::into)
        .next()
        .unwrap();
    assert!(cqe.result() >= 0);
    assert_eq!(cqe.user_data(), REQ_TYPE_PIPE);

    // Ensure the fds were assigned.
    assert_ne!(fds[0], -1);
    assert_ne!(fds[1], -1);

    // Wrap the raw fds.
    let mut rx = unsafe { PipeReader::from_raw_fd(fds[0]) };
    let mut tx = unsafe { PipeWriter::from_raw_fd(fds[1]) };

    // Write data to the pipe.
    for _ in 0..1024 {
        tx.write_all(DATA).unwrap();
    }

    // Read data from the pipe.
    let mut buf = [0u8; DATA.len() * 1024];
    rx.read_exact(&mut buf).unwrap();

    // Check the read data matches what was sent.
    for chunk in buf.chunks(DATA.len()) {
        assert_eq!(chunk, DATA);
    }

    Ok(())
}
