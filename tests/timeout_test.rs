use std::time::{ Instant, Duration };
use linux_io_uring::{ opcode, IoUring };


#[test]
fn test_timeout() -> anyhow::Result<()> {
    let mut io_uring = IoUring::new(1)?;

    let ts = opcode::KernelTimespec {
        tv_sec: 1,
        tv_nsec: 0
    };

    let entry = opcode::Timeout::new(&ts);
    unsafe {
        io_uring
            .submission()
            .available()
            .push(entry.build().user_data(0x42))
            .map_err(drop)
            .expect("queue is full");
    }

    let now = Instant::now();
    io_uring.submit_and_wait(1)?;
    let t = now.elapsed();
    assert_eq!(t.as_secs(), 1);

    let entry = io_uring
        .completion()
        .available()
        .next()
        .expect("queue is empty");

    assert_eq!(entry.user_data(), 0x42);
    assert_eq!(entry.result(), -libc::ETIME);

    Ok(())
}

#[test]
fn test_timeout_want() -> anyhow::Result<()> {
    let mut io_uring = IoUring::new(4)?;

    let ts = opcode::KernelTimespec {
        tv_sec: 1,
        tv_nsec: 0
    };

    // want many but timeout
    let entry = opcode::Timeout::new(&ts);
    unsafe {
        io_uring
            .submission()
            .available()
            .push(entry.build().user_data(0x42))
            .map_err(drop)
            .expect("queue is full");
    }

    let now = Instant::now();
    io_uring.submit_and_wait(2)?; // there won't be two events
    let t = now.elapsed();
    assert_eq!(t.as_secs(), 1);

    let cqes = io_uring
        .completion()
        .available()
        .cloned()
        .collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x42);
    assert_eq!(cqes[0].result(), -libc::ETIME);


    // want event buf timeout
    let (rp, _wp) = nix::unistd::pipe()?; // just test, we don't close

    let entry = opcode::Timeout::new(&ts);
    unsafe {
        io_uring
            .submission()
            .available()
            .push(entry.build().user_data(0x42))
            .map_err(drop)
            .expect("queue is full");
    }

    let now = Instant::now();
    io_uring.submit()?;

    let entry = opcode::PollAdd::new(opcode::Target::Fd(rp), libc::POLLIN);
    unsafe {
        io_uring
            .submission()
            .available()
            .push(entry.build().user_data(0x43))
            .map_err(drop)
            .expect("queue is full");
    }

    io_uring.submit_and_wait(2)?;
    let t = now.elapsed();
    assert_eq!(t.as_secs(), 1);

    let cqes = io_uring
        .completion()
        .available()
        .cloned()
        .collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x42);
    assert_eq!(cqes[0].result(), -libc::ETIME);

    Ok(())
}

#[test]
fn test_timeout_early() -> anyhow::Result<()> {
    let mut io_uring = IoUring::new(4)?;

    let ts = opcode::KernelTimespec {
        tv_sec: 1,
        tv_nsec: 0
    };

    // early
    unsafe {
        let mut sq = io_uring.submission().available();
        sq.push(opcode::Timeout::new(&ts).build().user_data(0x42))
            .map_err(drop)
            .expect("queue is full");
        sq.push(opcode::Nop::new().build().user_data(0x43))
            .map_err(drop)
            .expect("queue is full");
    }

    let now = Instant::now();
    io_uring.submit_and_wait(2)?;
    let t = now.elapsed();
    assert!(t < Duration::from_secs(1));

    let mut cqes = io_uring
        .completion()
        .available()
        .cloned()
        .collect::<Vec<_>>();
    cqes.sort_by_key(|entry| entry.user_data());

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].user_data(), 0x42);
    assert_eq!(cqes[1].user_data(), 0x43);
    assert_eq!(cqes[0].result(), 0);


    // early then timeout
    unsafe {
        io_uring
            .submission()
            .available()
            .push(opcode::Timeout::new(&ts).build().user_data(0x42))
            .map_err(drop)
            .expect("queue is full");

    }

    let now = Instant::now();
    io_uring.submit()?;

    unsafe {
        let mut sq = io_uring.submission().available();
        sq.push(opcode::Nop::new().build().user_data(0x43))
            .map_err(drop)
            .expect("queue is full");
        sq.push(opcode::Timeout::new(&ts).build().user_data(0x44))
            .map_err(drop)
            .expect("queue is full");
    }

    let now2 = Instant::now();
    io_uring.submit_and_wait(4)?;
    let t = now.elapsed();
    let t2 = now2.elapsed();

    assert_eq!(t2.as_secs(), 1);
    assert!(t > t2);

    let mut cqes = io_uring
        .completion()
        .available()
        .cloned()
        .collect::<Vec<_>>();
    cqes.sort_by_key(|entry| entry.user_data());

    assert_eq!(cqes.len(), 3);
    assert_eq!(cqes[0].user_data(), 0x42);
    assert_eq!(cqes[1].user_data(), 0x43);
    assert_eq!(cqes[2].user_data(), 0x44);
    assert_eq!(cqes[0].result(), 0);
    assert_eq!(cqes[2].result(), -libc::ETIME);

    Ok(())
}
