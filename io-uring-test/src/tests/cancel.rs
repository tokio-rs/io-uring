use crate::Test;
use io_uring::cqueue::EntryMarker;
use io_uring::types::CancelBuilder;
use io_uring::{cqueue, opcode, squeue, types, IoUring};
use std::fs::File;
use std::os::fd::FromRawFd;
use std::os::unix::io::AsRawFd;

// Cancels one request matching the user data.
pub fn test_async_cancel_user_data<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Timeout::CODE);
        test.probe.is_supported(opcode::AsyncCancel2::CODE);
    );

    println!("test async_cancel_user_data");

    let ts = types::Timespec::new().sec(1);
    let timeout_e = opcode::Timeout::new(&ts).build();

    // Cancel the timeout matching user data
    let builder = CancelBuilder::user_data(2003);
    let cancel_e = opcode::AsyncCancel2::new(builder).build();

    let entries = [
        timeout_e.user_data(2003).into(),
        cancel_e.user_data(2004).into(),
    ];
    for sqe in entries.clone() {
        unsafe {
            ring.submission().push(sqe).expect("queue is full");
        }
    }

    // Wait for both timeout and the cancel request.
    ring.submit_and_wait(entries.len())?;

    let mut cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
    cqes.sort_unstable_by_key(cqueue::Entry::user_data);

    assert_eq!(cqes.len(), entries.len());

    assert_eq!(cqes[0].user_data(), 2003);
    assert_eq!(cqes[1].user_data(), 2004);

    assert_eq!(cqes[0].result(), -libc::ECANCELED); // -ECANCELED
    assert_eq!(cqes[1].result(), 0); // the number of requests cancelled

    Ok(())
}

// Cancels one request matching the user data.
pub fn test_async_cancel_user_data_all<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Timeout::CODE);
        test.probe.is_supported(opcode::AsyncCancel2::CODE);
        test.probe.is_supported(opcode::Socket::CODE); // Check if Kernel >= 5.19
    );

    println!("test async_cancel_user_data_all");

    let ts = types::Timespec::new().sec(1);
    let timeout_e = opcode::Timeout::new(&ts).build();

    // Cancel all timeouts matching user data
    let builder = CancelBuilder::user_data(2003).all();
    let cancel_e = opcode::AsyncCancel2::new(builder).build();

    let entries = [
        timeout_e.clone().user_data(2003).into(),
        timeout_e.user_data(2003).into(),
        cancel_e.user_data(2004).into(),
    ];
    for sqe in entries.clone() {
        unsafe {
            ring.submission().push(sqe).expect("queue is full");
        }
    }

    // Wait for both timeouts and the cancel request.
    ring.submit_and_wait(entries.len())?;

    let mut cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
    cqes.sort_unstable_by_key(cqueue::Entry::user_data);

    assert_eq!(cqes.len(), entries.len());

    assert_eq!(cqes[0].user_data(), 2003);
    assert_eq!(cqes[1].user_data(), 2003);
    assert_eq!(cqes[2].user_data(), 2004);

    assert_eq!(cqes[0].result(), -libc::ECANCELED); // -ECANCELED
    assert_eq!(cqes[1].result(), -libc::ECANCELED); // -ECANCELED
    assert_eq!(cqes[2].result(), 2); // the number of requests cancelled

    Ok(())
}

// Cancels any requests.
pub fn test_async_cancel_any<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Timeout::CODE);
        test.probe.is_supported(opcode::AsyncCancel2::CODE);
        test.probe.is_supported(opcode::Socket::CODE); // Check if Kernel >= 5.19
    );

    println!("test async_cancel_any");

    let ts = types::Timespec::new().sec(1);
    let timeout_e = opcode::Timeout::new(&ts).build();

    // Cancel any pending requests
    let builder = CancelBuilder::any();
    let cancel_e = opcode::AsyncCancel2::new(builder).build();

    let entries = [
        timeout_e.clone().user_data(2003).into(),
        timeout_e.user_data(2004).into(),
        cancel_e.user_data(2005).into(),
    ];
    for sqe in entries.clone() {
        unsafe {
            ring.submission().push(sqe).expect("queue is full");
        }
    }

    // Wait for both timeouts and the cancel request.
    ring.submit_and_wait(entries.len())?;

    let mut cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
    cqes.sort_unstable_by_key(cqueue::Entry::user_data);

    assert_eq!(cqes.len(), entries.len());

    assert_eq!(cqes[0].user_data(), 2003);
    assert_eq!(cqes[1].user_data(), 2004);
    assert_eq!(cqes[2].user_data(), 2005);

    assert_eq!(cqes[0].result(), -libc::ECANCELED); // -ECANCELED
    assert_eq!(cqes[1].result(), -libc::ECANCELED);
    assert_eq!(cqes[2].result(), 2); // the number of requests cancelled

    Ok(())
}

pub fn test_async_cancel_fd<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::PollAdd::CODE);
        test.probe.is_supported(opcode::AsyncCancel2::CODE);
        test.probe.is_supported(opcode::Socket::CODE); // Check if Kernel >= 5.19
    );

    println!("test async_cancel_fd");

    let _fd = create_dummy_fd()?;
    let fd = types::Fd(_fd.as_raw_fd());
    let poll_e = opcode::PollAdd::new(fd, libc::POLLIN as _).build();

    // Cancel one poll request matching FD
    let builder = CancelBuilder::fd(fd);
    let cancel_e = opcode::AsyncCancel2::new(builder).build();

    let entries = [
        poll_e.user_data(2003).into(),
        cancel_e.user_data(2004).into(),
    ];
    for sqe in entries.clone() {
        unsafe {
            ring.submission().push(sqe).expect("queue is full");
        }
    }

    // Wait for 2 requests: canceled poll and the cancel request itself.
    ring.submit_and_wait(entries.len())?;

    let mut cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
    cqes.sort_unstable_by_key(cqueue::Entry::user_data);

    assert_eq!(cqes.len(), entries.len());

    assert_eq!(cqes[0].user_data(), 2003);
    assert_eq!(cqes[1].user_data(), 2004);

    assert_eq!(cqes[0].result(), -libc::ECANCELED); // -ECANCELED
    assert_eq!(cqes[1].result(), 0);

    Ok(())
}

// Cancels all pending requests with the given FD.
pub fn test_async_cancel_fd_all<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::PollAdd::CODE);
        test.probe.is_supported(opcode::AsyncCancel2::CODE);
        test.probe.is_supported(opcode::Socket::CODE); // Check if Kernel >= 5.19
    );

    println!("test async_cancel_fd_all");

    let _fd = create_dummy_fd()?;
    let fd = types::Fd(_fd.as_raw_fd());
    let poll_e = opcode::PollAdd::new(fd, libc::POLLIN as _).build();

    // Cancel all requests matching FD
    let builder = CancelBuilder::fd(fd).all();
    let cancel_e = opcode::AsyncCancel2::new(builder).build();

    let entries = [
        poll_e.clone().user_data(2003).into(),
        poll_e.user_data(2004).into(),
        cancel_e.user_data(2005).into(),
    ];
    for sqe in entries.clone() {
        unsafe {
            ring.submission().push(sqe).expect("queue is full");
        }
    }

    // Wait for both polls and the cancel request.
    ring.submit_and_wait(entries.len())?;

    let mut cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
    cqes.sort_unstable_by_key(cqueue::Entry::user_data);

    assert_eq!(cqes.len(), entries.len());

    assert_eq!(cqes[0].user_data(), 2003);
    assert_eq!(cqes[1].user_data(), 2004);
    assert_eq!(cqes[2].user_data(), 2005);

    assert_eq!(cqes[0].result(), -libc::ECANCELED); // -ECANCELED
    assert_eq!(cqes[1].result(), -libc::ECANCELED);
    assert_eq!(cqes[2].result(), 2); // the number of requests cancelled

    Ok(())
}

fn create_dummy_fd() -> anyhow::Result<File> {
    unsafe {
        let fd = libc::eventfd(0, libc::EFD_CLOEXEC);

        if fd == -1 {
            return Err(std::io::Error::last_os_error().into());
        }

        Ok(File::from_raw_fd(fd))
    }
}
