use io_uring::{opcode, types, IoUring, Probe};
use std::fs::File;
use std::io::{self, Write};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::thread;
use std::time::Duration;

pub fn test_eventfd_poll(ring: &mut IoUring, probe: &Probe) -> anyhow::Result<()> {
    require!(
        probe.is_supported(opcode::PollAdd::CODE);
    );

    println!("test eventfd_poll");

    let mut fd = unsafe {
        let fd = libc::eventfd(0, libc::EFD_CLOEXEC);

        if fd == -1 {
            return Err(io::Error::last_os_error().into());
        }

        File::from_raw_fd(fd)
    };

    let poll_e = opcode::PollAdd::new(types::Fd(fd.as_raw_fd()), libc::POLLIN as _);

    unsafe {
        let mut queue = ring.submission();
        queue
            .push(&poll_e.build().user_data(0x04))
            .expect("queue is full");
    }

    ring.submit()?;
    thread::sleep(Duration::from_millis(200));
    assert_eq!(ring.completion().len(), 0);

    fd.write_all(&0x1u64.to_ne_bytes())?;
    ring.submit_and_wait(1)?;

    let cqes = ring.completion().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x04);
    assert_eq!(cqes[0].result(), 1);

    Ok(())
}

pub fn test_eventfd_poll_remove(ring: &mut IoUring, probe: &Probe) -> anyhow::Result<()> {
    require!(
        probe.is_supported(opcode::PollAdd::CODE);
        probe.is_supported(opcode::PollRemove::CODE);
    );

    println!("test eventfd_poll_remove");

    let mut fd = unsafe {
        let fd = libc::eventfd(0, libc::EFD_CLOEXEC);

        if fd == -1 {
            return Err(io::Error::last_os_error().into());
        }

        File::from_raw_fd(fd)
    };

    // add poll

    let poll_e = opcode::PollAdd::new(types::Fd(fd.as_raw_fd()), libc::POLLIN as _);

    unsafe {
        let mut queue = ring.submission();
        queue
            .push(&poll_e.build().user_data(0x05))
            .expect("queue is full");
    }

    ring.submit()?;

    // remove poll

    let poll_e = opcode::PollRemove::new(0x05);

    unsafe {
        let mut queue = ring.submission();
        queue
            .push(&poll_e.build().user_data(0x06))
            .expect("queue is full");
    }

    ring.submit()?;

    thread::sleep(Duration::from_millis(200));

    fd.write_all(&0x1u64.to_ne_bytes())?;
    ring.submit_and_wait(2)?;

    let mut cqes = ring.completion().collect::<Vec<_>>();
    cqes.sort_by_key(|cqe| cqe.user_data());

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].user_data(), 0x05);
    assert_eq!(cqes[1].user_data(), 0x06);
    assert_eq!(cqes[0].result(), -libc::ECANCELED);
    assert_eq!(cqes[1].result(), 0);

    Ok(())
}

pub fn test_eventfd_poll_remove_failed(ring: &mut IoUring, probe: &Probe) -> anyhow::Result<()> {
    require!(
        probe.is_supported(opcode::PollAdd::CODE);
        probe.is_supported(opcode::PollRemove::CODE);
    );

    println!("test eventfd_poll_remove_failed");

    let mut fd = unsafe {
        let fd = libc::eventfd(0, libc::EFD_CLOEXEC);

        if fd == -1 {
            return Err(io::Error::last_os_error().into());
        }

        File::from_raw_fd(fd)
    };

    // add poll

    let poll_e = opcode::PollAdd::new(types::Fd(fd.as_raw_fd()), libc::POLLIN as _);

    unsafe {
        let mut queue = ring.submission();
        queue
            .push(&poll_e.build().user_data(0x07))
            .expect("queue is full");
    }

    fd.write_all(&0x1u64.to_ne_bytes())?;
    ring.submit_and_wait(1)?;
    assert_eq!(ring.completion().len(), 1);

    // remove poll

    let poll_e = opcode::PollRemove::new(0x08);

    unsafe {
        let mut queue = ring.submission();
        queue
            .push(&poll_e.build().user_data(0x08))
            .expect("queue is full");
    }

    ring.submit_and_wait(2)?;

    let mut cqes = ring.completion().collect::<Vec<_>>();
    cqes.sort_by_key(|cqe| cqe.user_data());

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].user_data(), 0x07);
    assert_eq!(cqes[1].user_data(), 0x08);
    assert_eq!(cqes[0].result(), 1);
    assert_eq!(cqes[1].result(), -libc::ENOENT);

    Ok(())
}
