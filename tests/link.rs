mod common;

use std::{ io, thread };
use std::sync::Arc;
use std::convert::TryInto;
use std::time::{ Instant, Duration };
use std::os::unix::io::AsRawFd;
use nix::sys::eventfd::*;
use io_uring::opcode::{ self, types };
use io_uring::{ squeue, IoUring };
use common::{ Fd, is_stable_kernel };


#[test]
fn test_nop_link() -> anyhow::Result<()> {
    let mut ring = IoUring::new(4)?;

    unsafe {
        let mut sq = ring.submission().available();
        sq.push(opcode::Nop::new().build().user_data(0x01).flags(squeue::Flags::IO_LINK))
            .ok()
            .expect("queue is full");
        sq.push(opcode::Nop::new().build().user_data(0x02).flags(squeue::Flags::IO_LINK))
            .ok()
            .expect("queue is full");
        sq.push(opcode::Nop::new().build().user_data(0x03).flags(squeue::Flags::IO_LINK))
            .ok()
            .expect("queue is full");
        sq.push(opcode::Nop::new().build().user_data(0x04))
            .ok()
            .expect("queue is full");
    }

    ring.submit_and_wait(4)?;

    let cqes = ring
        .completion()
        .available()
        .map(|entry| entry.user_data())
        .collect::<Vec<_>>();

    assert_eq!(cqes, [0x01, 0x02, 0x03, 0x04]);

    Ok(())
}

#[test]
fn test_link_cross_submit() -> anyhow::Result<()> {
    let mut ring = IoUring::new(4)?;
    let efd: Arc<Fd> = eventfd(0, EfdFlags::EFD_CLOEXEC)?
        .try_into()
        .map(Arc::new)
        .map_err(|_| anyhow::format_err!("invalid fd"))?;
    let efd2 = efd.clone();

    let mut buf = [0; 8];
    let mut bufs = [io::IoSliceMut::new(&mut buf)];
    let entry = {
        let efd = types::Target::Fd(efd.as_raw_fd());
        let bufs_ptr = bufs.as_mut_ptr() as *mut _;
        opcode::Readv::new(efd, bufs_ptr, 1)
    };

    unsafe {
        let mut sq = ring.submission().available();
        sq.push(entry.build().user_data(0x01).flags(squeue::Flags::IO_LINK))
            .ok()
            .expect("queue is full");
    }

    let h = thread::spawn(move || {
        thread::sleep(Duration::from_secs(1));
        nix::unistd::write(efd2.as_raw_fd(), &0x1u64.to_le_bytes())?;
        Ok(()) as anyhow::Result<()>
    });

    let now = Instant::now();
    ring.submit()?;

    unsafe {
        let mut sq = ring.submission().available();
        sq.push(opcode::Nop::new().build().user_data(0x02))
            .ok()
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;
    assert_eq!(now.elapsed().as_secs(), 0);
    assert_eq!(ring.completion().len(), 1);

    ring.submit_and_wait(2)?;
    assert_eq!(now.elapsed().as_secs(), 1);

    let mut cqes = ring
        .completion()
        .available()
        .collect::<Vec<_>>();
    cqes.sort_by_key(|cqe| cqe.user_data());

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].user_data(), 0x01);
    assert_eq!(cqes[1].user_data(), 0x02);

    h.join().unwrap()?;

    Ok(())
}

#[test]
fn test_fail_link() -> anyhow::Result<()> {
    let mut ring = IoUring::new(4)?;

    let mut buf = [];
    let mut bufs = [io::IoSliceMut::new(&mut buf)];
    let badr_e = opcode::Readv::new(
        types::Target::Fd(-1),
        bufs.as_mut_ptr() as *mut _,
        0
    );

    unsafe {
        let mut sq = ring.submission().available();
        sq.push(badr_e.build().flags(squeue::Flags::IO_LINK).user_data(0x42))
            .ok()
            .expect("queue is full");
        sq.push(opcode::Nop::new().build().user_data(0x43))
            .ok()
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let mut cqes = ring
        .completion()
        .available()
        .collect::<Vec<_>>();
    cqes.sort_by_key(|cqe| cqe.user_data());

    if is_stable_kernel(&ring) {
        assert_eq!(cqes.len(), 2);
        assert_eq!(cqes[0].result(), -libc::EBADF);
        assert_eq!(cqes[0].user_data(), 0x42);
        assert_eq!(cqes[1].result(), 0);
        assert_eq!(cqes[1].user_data(), 0x43);
    } else {
        assert_eq!(cqes.len(), 1);
        assert_eq!(cqes[0].result(), -libc::EBADF);
        assert_eq!(cqes[0].user_data(), 0x42);
    }

    Ok(())
}

#[test]
fn test_fail_link_cross_submit() -> anyhow::Result<()> {
    let mut ring = IoUring::new(4)?;

    let mut buf = [0];
    let mut bufs = [io::IoSliceMut::new(&mut buf)];
    let badr_e = opcode::Readv::new(
        types::Target::Fd(-1),
        bufs.as_mut_ptr() as *mut _,
        0
    );

    unsafe {
        let mut sq = ring.submission().available();
        sq.push(badr_e.build().flags(squeue::Flags::IO_LINK).user_data(0x42))
            .ok()
            .expect("queue is full");
    }

    ring.submit()?;

    unsafe {
        let mut sq = ring.submission().available();
        sq.push(opcode::Nop::new().build().user_data(0x43))
            .ok()
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let mut cqes = ring
        .completion()
        .available()
        .collect::<Vec<_>>();
    cqes.sort_by_key(|cqe| cqe.user_data());

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].result(), -libc::EBADF);
    assert_eq!(cqes[0].user_data(), 0x42);
    assert_eq!(cqes[1].user_data(), 0x43);

    Ok(())
}
