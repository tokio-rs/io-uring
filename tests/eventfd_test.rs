use std::{ io, thread };
use std::time::{ Duration, Instant };
use nix::sys::eventfd::*;
use linux_io_uring::{ opcode, IoUring };


#[test]
fn test_eventfd() -> anyhow::Result<()> {
    let mut io_uring = IoUring::new(1)?;
    let efd = eventfd(0, EfdFlags::EFD_CLOEXEC | EfdFlags::EFD_NONBLOCK)?; // just test, we don't close

    // write then poll
    let entry = opcode::PollAdd::new(opcode::Target::Fd(efd), libc::POLLIN);
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

    nix::unistd::write(efd, &0x1u64.to_le_bytes())?;
    io_uring.submit_and_wait(1)?;
    let entry = io_uring
        .completion()
        .available()
        .next()
        .expect("queue is empty");
    assert_eq!(entry.result() as i16 & libc::POLLIN, libc::POLLIN);
    assert_eq!(entry.user_data(), 0x42);

    // poll
    let entry = opcode::PollAdd::new(opcode::Target::Fd(efd), libc::POLLIN);
    unsafe {
        io_uring
            .submission()
            .available()
            .push(entry.build().user_data(0x43))
            .map_err(drop)
            .expect("queue is full");
    }

    io_uring.submit_and_wait(1)?;
    let entry = io_uring
        .completion()
        .available()
        .next()
        .expect("queue is empty");
    assert_eq!(entry.result() as i16 & libc::POLLIN, libc::POLLIN);
    assert_eq!(entry.user_data(), 0x43);

    // read then poll
    let mut buf = [0; 8];
    nix::unistd::read(efd, &mut buf)?;

    let entry = opcode::PollAdd::new(opcode::Target::Fd(efd), libc::POLLIN);
    unsafe {
        io_uring
            .submission()
            .available()
            .push(entry.build().user_data(0x44))
            .map_err(drop)
            .expect("queue is full");
    }

    let now = Instant::now();
    let handle = thread::spawn(move || {
        thread::sleep(Duration::from_secs(1));
        nix::unistd::write(efd, &0x1u64.to_le_bytes())?;
        Ok(()) as anyhow::Result<()>
    });

    io_uring.submit_and_wait(1)?;
    let t = now.elapsed();
    assert_eq!(t.as_secs(), 1);

    let entry = io_uring
        .completion()
        .available()
        .next()
        .expect("queue is empty");
    assert_eq!(entry.result() as i16 & libc::POLLIN, libc::POLLIN);
    assert_eq!(entry.user_data(), 0x44);

    handle.join().unwrap()?;

    Ok(())
}

#[test]
fn test_eventfd_blocking() -> anyhow::Result<()> {
    let mut io_uring = IoUring::new(1)?;
    let efd = eventfd(0, EfdFlags::EFD_CLOEXEC)?; // just test, we don't close

    let mut buf = [0; 8];
    let mut bufs = [io::IoSliceMut::new(&mut buf)];
    let entry = opcode::Readv::new(
        opcode::Target::Fd(efd),
        bufs.as_mut_ptr() as *mut _,
        1
    );

    unsafe {
        io_uring
            .submission()
            .available()
            .push(entry.build().user_data(0x43))
            .map_err(drop)
            .expect("queue is full");
    }
    io_uring.submit()?;

    let now = Instant::now();
    let handle = thread::spawn(move || {
        thread::sleep(Duration::from_secs(1));
        nix::unistd::write(efd, &0x1u64.to_le_bytes())?;
        Ok(()) as anyhow::Result<()>
    });

    io_uring.submit_and_wait(1)?;
    let t = now.elapsed();
    assert_eq!(t.as_secs(), 1);

    let entry = io_uring
        .completion()
        .available()
        .next()
        .expect("queue is empty");
    assert_eq!(u64::from_le_bytes(buf), 1);
    assert_eq!(entry.user_data(), 0x43);

    handle.join().unwrap()?;

    Ok(())
}
