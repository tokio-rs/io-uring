use linux_io_uring::{opcode, IoUring};

#[test]
fn test_poll_add() -> anyhow::Result<()> {
    let mut io_uring = IoUring::new(1)?;
    let (rp, wp) = nix::unistd::pipe()?; // just test, we don't close

    let entry = opcode::PollAdd::new(opcode::Target::Fd(rp), libc::POLLIN);
    unsafe {
        io_uring
            .submission()
            .available()
            .push(entry.build().user_data(0x42))
            .map_err(drop)
            .expect("queue is full");
    }

    io_uring.submit()?;
    assert!(io_uring.completion().is_empty());

    nix::unistd::write(wp, b"pipe")?;
    io_uring.submit_and_wait(1)?;
    let entry = io_uring
        .completion()
        .available()
        .next()
        .expect("queue is empty");
    assert_eq!(entry.result() as i16 & libc::POLLIN, libc::POLLIN);
    assert_eq!(entry.user_data(), 0x42);

    Ok(())
}

#[test]
fn test_poll_remove() -> anyhow::Result<()> {
    let mut io_uring = IoUring::new(1)?;
    let (rp, _) = nix::unistd::pipe()?; // just test, we don't close

    let token = 0x43;

    let entry = opcode::PollAdd::new(opcode::Target::Fd(rp), libc::POLLIN);
    unsafe {
        io_uring
            .submission()
            .available()
            .push(entry.build().user_data(token))
            .map_err(drop)
            .expect("queue is full");
    }

    io_uring.submit()?;
    assert!(io_uring.completion().is_empty());

    let entry = opcode::PollRemove::new(token);
    unsafe {
        io_uring
            .submission()
            .available()
            .push(entry.build().user_data(token))
            .map_err(drop)
            .expect("queue is full");
    }

    io_uring.submit_and_wait(1)?;

    let entry = io_uring
        .completion()
        .available()
        .next()
        .expect("queue is empty");
    assert_eq!(entry.result(), 0);
    assert_eq!(entry.user_data(), token);

    Ok(())
}
