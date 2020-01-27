mod common;

use std::io;
use std::time::Instant;
use std::convert::TryInto;
use std::os::unix::io::AsRawFd;
use io_uring::opcode::{ self, types };
use io_uring::{ squeue, IoUring };
use common::Fd;


const TS1: types::Timespec = types::Timespec { tv_sec: 1, tv_nsec: 0 };
const TS2: types::Timespec = types::Timespec { tv_sec: 2, tv_nsec: 0 };

#[test]
fn test_timeout() -> anyhow::Result<()> {
    let mut ring = IoUring::new(1)?;

    let entry = opcode::Timeout::new(&TS1);
    unsafe {
        ring
            .submission()
            .available()
            .push(entry.build().user_data(0x42))
            .ok()
            .expect("queue is full");
    }

    let now = Instant::now();
    ring.submit_and_wait(1)?;
    let t = now.elapsed();
    assert_eq!(t.as_secs(), 1);

    let entry = ring
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
    let mut ring = IoUring::new(4)?;

    // want more but timeout
    let entry = opcode::Timeout::new(&TS1);
    unsafe {
        ring
            .submission()
            .available()
            .push(entry.build().user_data(0x42))
            .map_err(drop)
            .expect("queue is full");
    }

    let now = Instant::now();
    ring.submit_and_wait(2)?; // there won't be two events
    let t = now.elapsed();
    assert_eq!(t.as_secs(), 1);

    let cqes = ring
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
        ring
            .submission()
            .available()
            .push(entry.build().user_data(0x42))
            .map_err(drop)
            .expect("queue is full");
    }

    let now = Instant::now();
    ring.submit()?;

    let entry = opcode::PollAdd::new(
        types::Target::Fd(rp.as_raw_fd()),
        libc::POLLIN
    );
    unsafe {
        ring
            .submission()
            .available()
            .push(entry.build().user_data(0x43))
            .map_err(drop)
            .expect("queue is full");
    }

    ring.submit_and_wait(2)?;
    let t = now.elapsed();
    assert_eq!(t.as_secs(), 1);

    let cqes = ring
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
    let mut ring = IoUring::new(4)?;

    unsafe {
        let mut sq = ring.submission().available();
        sq.push(opcode::Nop::new().build().user_data(0x42))
            .map_err(drop)
            .expect("queue is full");
        sq.push(opcode::Timeout::new(&TS1).build().user_data(0x43))
            .map_err(drop)
            .expect("queue is full");
    }

    let now = Instant::now();
    ring.submit_and_wait(1)?;
    let t = now.elapsed();
    assert_eq!(t.as_secs(), 0);

    let cqes = ring
        .completion()
        .available()
        .collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x42);

    ring.submit_and_wait(1)?;
    let t = now.elapsed();
    assert_eq!(t.as_secs(), 1);

    let cqes = ring
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
    let mut ring = IoUring::new(4)?;

    unsafe {
        let mut sq = ring.submission().available();
        sq.push(opcode::Nop::new().build().user_data(0x42))
            .map_err(drop)
            .expect("queue is full");
        sq.push(opcode::Timeout::new(&TS1).build().user_data(0x43))
            .map_err(drop)
            .expect("queue is full");
    }

    let now = Instant::now();
    ring.submit_and_wait(1)?;
    let t = now.elapsed();
    assert_eq!(t.as_secs(), 0);

    let cqes = ring
        .completion()
        .available()
        .collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x42);

    unsafe {
        let mut sq = ring.submission().available();
        sq.push(opcode::Timeout::new(&TS2).build().user_data(0x44))
            .map_err(drop)
            .expect("queue is full");
    }

    let now2 = Instant::now();
    ring.submit_and_wait(1)?;
    let t = now.elapsed();
    assert_eq!(t.as_secs(), 1);

    let cqes = ring
        .completion()
        .available()
        .collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x43);
    assert_eq!(cqes[0].result(), -libc::ETIME);


    ring.submit_and_wait(1)?;
    let t = now2.elapsed();
    assert_eq!(t.as_secs(), 2);

    let cqes = ring
        .completion()
        .available()
        .collect::<Vec<_>>();

    assert_eq!(cqes[0].user_data(), 0x44);
    assert_eq!(cqes[0].result(), -libc::ETIME);

    Ok(())
}

#[test]
fn test_timeout_remove() -> anyhow::Result<()> {
    let mut ring = IoUring::new(4)?;

    let timeout_e = opcode::Timeout::new(&TS1)
        .build()
        .user_data(0x01);
    let remove_e = opcode::TimeoutRemove::new(0x01)
        .build()
        .user_data(0x02);

    unsafe {
        ring.submission()
            .available()
            .push(timeout_e)
            .ok()
            .expect("queue is full");
    }

    let now = Instant::now();
    ring.submit()?;

    unsafe {
        ring.submission()
            .available()
            .push(remove_e)
            .ok()
            .expect("queue is full");
    }

    ring.submit_and_wait(2)?;
    assert_eq!(now.elapsed().as_secs(), 0);

    let mut cqes = ring
        .completion()
        .available()
        .collect::<Vec<_>>();
    cqes.sort_by_key(|cqe| cqe.user_data());

    assert_eq!(cqes[0].user_data(), 0x01);
    assert_eq!(cqes[0].result(), -libc::ECANCELED);
    assert_eq!(cqes[1].user_data(), 0x02);
    assert_eq!(cqes[1].result(), 0);

    Ok(())
}

#[test]
fn test_timeout_remove_link() -> anyhow::Result<()> {
    let mut ring = IoUring::new(4)?;

    let timeout_e = opcode::Timeout::new(&TS2)
        .build()
        .user_data(0x01);
    let remove_e = opcode::TimeoutRemove::new(0x01)
        .build()
        .user_data(0x02);
    let timeout2_e = opcode::Timeout::new(&TS1)
        .build()
        .user_data(0x01);

    unsafe {
        ring.submission()
            .available()
            .push(timeout_e)
            .ok()
            .expect("queue is full");
    }

    let now = Instant::now();
    ring.submit()?;

    unsafe {
        let mut sq = ring.submission().available();
        sq.push(remove_e)
            .ok()
            .expect("queue is full");
        sq.push(timeout2_e)
            .ok()
            .expect("queue is full");
    }

    ring.submit_and_wait(3)?;
    assert_eq!(now.elapsed().as_secs(), 1);

    let mut cqes = ring
        .completion()
        .available()
        .collect::<Vec<_>>();
    cqes.sort_by_key(|cqe| cqe.user_data());

    assert_eq!(cqes[0].user_data(), 0x01);
    assert_eq!(cqes[1].user_data(), 0x01);
    assert!(cqes[0].result() == -libc::ECANCELED || cqes[0].result() == -libc::ETIME);
    assert!(cqes[1].result() == -libc::ECANCELED || cqes[1].result() == -libc::ETIME);
    assert_ne!(cqes[0].result(), cqes[1].result());
    assert_eq!(cqes[1].result(), -libc::ETIME);
    assert_eq!(cqes[2].user_data(), 0x02);
    assert_eq!(cqes[2].result(), 0);

    Ok(())
}

#[test]
fn test_link_timeout() -> anyhow::Result<()> {
    use nix::sys::eventfd::*;

    let mut ring = IoUring::new(4)?;
    let efd: Fd = eventfd(0, EfdFlags::EFD_CLOEXEC)?
        .try_into()
        .map_err(|_| anyhow::format_err!("invalid fd"))?;

    nix::unistd::write(efd.as_raw_fd(), &0x1u64.to_le_bytes())?;

    let mut buf = [0; 8];
    let mut bufs = [io::IoSliceMut::new(&mut buf)];
    let readv_e = {
        let efd = types::Target::Fd(efd.as_raw_fd());
        let bufs_ptr = bufs.as_mut_ptr() as *mut _;
        opcode::Readv::new(efd, bufs_ptr, 1)
            .build()
            .user_data(0x01)
            .flags(squeue::Flags::IO_LINK)
    };
    let timeout_e = opcode::LinkTimeout::new(&TS1)
        .build()
        .user_data(0x02)
        .flags(squeue::Flags::IO_LINK);
    let nop_e = opcode::Nop::new()
        .build()
        .user_data(0x03);

    unsafe {
        let mut sq = ring.submission().available();
        sq.push(readv_e)
            .ok()
            .expect("queue is full");
        sq.push(timeout_e)
            .ok()
            .expect("queue is full");
        sq.push(nop_e)
            .ok()
            .expect("queue is full");
    }

    let now = Instant::now();
    ring.submit_and_wait(3)?;
    assert_eq!(now.elapsed().as_secs(), 0);

    let mut cqes = ring
        .completion()
        .available()
        .collect::<Vec<_>>();

    cqes.sort_by_key(|cqe| cqe.user_data());

    assert_eq!(cqes.len(), 3);
    assert_eq!(cqes[0].user_data(), 0x01);
    assert_eq!(cqes[0].result(), 8);
    assert_eq!(cqes[1].user_data(), 0x02);
    assert_eq!(cqes[1].result(), -libc::ECANCELED);
    assert_eq!(cqes[2].user_data(), 0x03);
    assert_eq!(cqes[2].result(), 0);

    Ok(())
}

#[test]
fn test_link_timeout_cancel() -> anyhow::Result<()> {
    use nix::sys::eventfd::*;

    let mut ring = IoUring::new(4)?;
    let efd: Fd = eventfd(0, EfdFlags::EFD_CLOEXEC)?
        .try_into()
        .map_err(|_| anyhow::format_err!("invalid fd"))?;

    let mut buf = [0; 8];
    let mut bufs = [io::IoSliceMut::new(&mut buf)];
    let readv_e = {
        let efd = types::Target::Fd(efd.as_raw_fd());
        let bufs_ptr = bufs.as_mut_ptr() as *mut _;
        opcode::Readv::new(efd, bufs_ptr, 1)
            .build()
            .user_data(0x01)
            .flags(squeue::Flags::IO_LINK)
    };
    let timeout_e = opcode::LinkTimeout::new(&TS1)
        .build()
        .user_data(0x02)
        .flags(squeue::Flags::IO_LINK);
    let nop_e = opcode::Nop::new()
        .build()
        .user_data(0x03);

    unsafe {
        let mut sq = ring.submission().available();
        sq.push(readv_e)
            .ok()
            .expect("queue is full");
        sq.push(timeout_e)
            .ok()
            .expect("queue is full");
        sq.push(nop_e)
            .ok()
            .expect("queue is full");
    }

    let now = Instant::now();
    ring.submit_and_wait(3)?;
    assert_eq!(now.elapsed().as_secs(), 1);

    let mut cqes = ring
        .completion()
        .available()
        .collect::<Vec<_>>();

    cqes.sort_by_key(|cqe| cqe.user_data());

    assert_eq!(cqes.len(), 3);
    assert_eq!(cqes[0].user_data(), 0x01);
    assert!(cqes[0].result() == -libc::EINTR || cqes[0].result() == -libc::ECANCELED);
    assert_eq!(cqes[1].user_data(), 0x02);
    assert_eq!(cqes[1].result(), -libc::EALREADY);
    assert_eq!(cqes[2].user_data(), 0x03);
    assert_eq!(cqes[2].result(), -libc::ECANCELED);

    Ok(())
}
