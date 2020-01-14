mod common;

use std::time::Instant;
use std::os::unix::io::AsRawFd;
use io_uring::opcode::{ self, types };
use io_uring::IoUring;
use common::Fd;


const TS1: types::Timespec = types::Timespec { tv_sec: 1, tv_nsec: 0 };
const TS2: types::Timespec = types::Timespec { tv_sec: 2, tv_nsec: 0 };

#[test]
fn test_timeout() -> anyhow::Result<()> {
    let mut io_uring = IoUring::new(1)?;

    let entry = opcode::Timeout::new(&TS1);
    unsafe {
        io_uring
            .submission()
            .available()
            .push(entry.build().user_data(0x42))
            .ok()
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

    // want more but timeout
    let entry = opcode::Timeout::new(&TS1);
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
        .collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x42);
    assert_eq!(cqes[0].result(), -libc::ETIME);


    // want event but timeout
    let (rp, wp) = nix::unistd::pipe()?;
    let (rp, _wp) = (Fd(rp), Fd(wp));

    let entry = opcode::Timeout::new(&TS1);
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

    let entry = opcode::PollAdd::new(
        types::Target::Fd(rp.as_raw_fd()),
        libc::POLLIN
    );
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
        .collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x42);
    assert_eq!(cqes[0].result(), -libc::ETIME);

    Ok(())
}

#[test]
fn test_timeout_park() -> anyhow::Result<()> {
    let mut io_uring = IoUring::new(4)?;

    unsafe {
        let mut sq = io_uring.submission().available();
        sq.push(opcode::Nop::new().build().user_data(0x42))
            .map_err(drop)
            .expect("queue is full");
        sq.push(opcode::Timeout::new(&TS1).build().user_data(0x43))
            .map_err(drop)
            .expect("queue is full");
    }

    let now = Instant::now();
    io_uring.submit_and_wait(1)?;
    let t = now.elapsed();
    assert_eq!(t.as_secs(), 0);

    let cqes = io_uring
        .completion()
        .available()
        .collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x42);

    io_uring.submit_and_wait(1)?;
    let t = now.elapsed();
    assert_eq!(t.as_secs(), 1);

    let cqes = io_uring
        .completion()
        .available()
        .collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x43);
    assert_eq!(cqes[0].result(), -libc::ETIME);

    Ok(())
}

#[test]
fn test_timeout_park2() -> anyhow::Result<()> {
    let mut io_uring = IoUring::new(4)?;

    unsafe {
        let mut sq = io_uring.submission().available();
        sq.push(opcode::Nop::new().build().user_data(0x42))
            .map_err(drop)
            .expect("queue is full");
        sq.push(opcode::Timeout::new(&TS1).build().user_data(0x43))
            .map_err(drop)
            .expect("queue is full");
    }

    let now = Instant::now();
    io_uring.submit_and_wait(1)?;
    let t = now.elapsed();
    assert_eq!(t.as_secs(), 0);

    let cqes = io_uring
        .completion()
        .available()
        .collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x42);

    unsafe {
        let mut sq = io_uring.submission().available();
        sq.push(opcode::Timeout::new(&TS2).build().user_data(0x44))
            .map_err(drop)
            .expect("queue is full");
    }

    let now2 = Instant::now();
    io_uring.submit_and_wait(1)?;
    let t = now.elapsed();
    assert_eq!(t.as_secs(), 1);

    let cqes = io_uring
        .completion()
        .available()
        .collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x43);
    assert_eq!(cqes[0].result(), -libc::ETIME);


    io_uring.submit_and_wait(1)?;
    let t = now2.elapsed();
    assert_eq!(t.as_secs(), 2);

    let cqes = io_uring
        .completion()
        .available()
        .collect::<Vec<_>>();

    assert_eq!(cqes[0].user_data(), 0x44);
    assert_eq!(cqes[0].result(), -libc::ETIME);

    Ok(())
}
