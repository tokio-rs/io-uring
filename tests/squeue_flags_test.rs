use linux_io_uring::{ opcode, squeue, IoUring, Builder };


#[test]
fn test_io_drain() -> anyhow::Result<()> {
    let mut io_uring = IoUring::new(4)?;

    unsafe {
        let mut queue = io_uring.submission().available();
        queue.push(opcode::Nop::new().build().user_data(0x01))
            .map_err(drop)
            .expect("queue is full");
        queue.push(opcode::Nop::new().build().user_data(0x02))
            .map_err(drop)
            .expect("queue is full");
        queue.push(opcode::Nop::new().build().user_data(0x03).flags(squeue::Flag::IO_DRAIN))
            .map_err(drop)
            .expect("queue is full");
        queue.push(opcode::Nop::new().build().user_data(0x04))
            .map_err(drop)
            .expect("queue is full");
    }

    io_uring.submit_and_wait(4)?;

    let cqes = io_uring
        .completion()
        .available()
        .map(|entry| entry.user_data())
        .collect::<Vec<_>>();

    assert!(cqes[0] == 0x01 || cqes[0] == 0x02);
    assert!(cqes[1] == 0x01 || cqes[1] == 0x02);
    assert_ne!(cqes[0], cqes[1]);
    assert_eq!(cqes[2], 0x03);
    assert_eq!(cqes[3], 0x04);

    Ok(())
}

#[test]
fn test_io_link() -> anyhow::Result<()> {
    let mut io_uring = IoUring::new(4)?;

    unsafe {
        let mut queue = io_uring.submission().available();
        queue.push(opcode::Nop::new().build().user_data(0x01).flags(squeue::Flag::IO_LINK))
            .map_err(drop)
            .expect("queue is full");
        queue.push(opcode::Nop::new().build().user_data(0x02).flags(squeue::Flag::IO_LINK))
            .map_err(drop)
            .expect("queue is full");
        queue.push(opcode::Nop::new().build().user_data(0x03).flags(squeue::Flag::IO_LINK))
            .map_err(drop)
            .expect("queue is full");
        queue.push(opcode::Nop::new().build().user_data(0x04))
            .map_err(drop)
            .expect("queue is full");
    }

    io_uring.submit_and_wait(4)?;

    let cqes = io_uring
        .completion()
        .available()
        .map(|entry| entry.user_data())
        .collect::<Vec<_>>();

    assert_eq!(cqes, [0x01, 0x02, 0x03, 0x04]);

    Ok(())
}

#[test]
fn test_io_drain_link_empty() -> anyhow::Result<()> {
    let mut io_uring = IoUring::new(2)?;

    unsafe {
        io_uring
            .submission()
            .available()
            .push(opcode::Nop::new().build().user_data(0x42).flags(squeue::Flag::IO_DRAIN))
            .map_err(drop)
            .expect("queue is full");
    }

    io_uring.submit_and_wait(1)?;

    let entry = io_uring
        .completion()
        .available()
        .next()
        .expect("queue is empty");

    assert_eq!(entry.user_data(), 0x42);
    assert_eq!(entry.result(), 0);

    unsafe {
        io_uring
            .submission()
            .available()
            .push(opcode::Nop::new().build().user_data(0x43).flags(squeue::Flag::IO_LINK))
            .map_err(drop)
            .expect("queue is full");
    }

    io_uring.submit_and_wait(1)?;

    let entry = io_uring
        .completion()
        .available()
        .next()
        .expect("queue is empty");

    assert_eq!(entry.user_data(), 0x43);
    assert_eq!(entry.result(), 0);

    Ok(())
}

#[test]
fn test_iopoll_link_invalid() -> anyhow::Result<()> {
    let mut io_uring = Builder::new(2)
        .setup_iopoll()
        .build()?;

    unsafe {
        let mut sq = io_uring.submission().available();
        sq.push(opcode::Nop::new().build().user_data(0x42).flags(squeue::Flag::IO_LINK))
            .map_err(drop)
            .expect("queue is full");
        sq.push(opcode::Nop::new().build().user_data(0x43))
            .map_err(drop)
            .expect("queue is full");
    }

    io_uring.submit_and_wait(2)?;

    let cqes = io_uring
        .completion()
        .available()
        .collect::<Vec<_>>();

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].user_data(), 0x42);
    assert_eq!(cqes[0].result(), -libc::EINVAL);
    assert_eq!(cqes[1].result(), -libc::ECANCELED);

    Ok(())
}
