use std::{ io, mem };
use nix::sys::eventfd::*;
use linux_io_uring::{ opcode, IoUring, reg, unreg };


#[test]
fn test_fixed_file() -> anyhow::Result<()> {
    let text = b"hello world!";

    let mut io_uring = IoUring::new(1)?;
    let (rp, wp) = nix::unistd::pipe()?; // just test, we don't close

    // register fd
    io_uring.register(reg::Target::File(&[rp, wp]))?;

    // write fixed fd
    let bufs = [io::IoSlice::new(text)];
    let entry = opcode::Writev::new(
        opcode::Target::Fixed(1),
        bufs.as_ptr() as *const _,
        1
    );

    unsafe {
        io_uring
            .submission()
            .available()
            .push(entry.build().user_data(0x42))
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

    let mut buf = [0; 12];
    assert_eq!(text.len(), buf.len());
    nix::unistd::read(rp, &mut buf)?;

    assert_eq!(text, &buf);

    // unregister
    io_uring.unregister(unreg::Target::File)?;

    Ok(())
}

#[test]
fn test_fixed_buffer() -> anyhow::Result<()> {
    let text = b"hello world!";

    let mut io_uring = IoUring::new(1)?;
    let (rp, wp) = nix::unistd::pipe()?; // just test, we don't close

    let mut buf = vec![0; text.len()];

    // register buffer
    unsafe {
        let buf = io::IoSliceMut::new(&mut buf);
        io_uring.register(reg::Target::Buffer(&[mem::transmute(buf)]))?;
    }

    nix::unistd::write(wp, text)?;

    // read fixed
    let entry = opcode::ReadFixed::new(
        opcode::Target::Fd(rp),
        buf.as_mut_ptr(),
        buf.len() as _,
        0
    );

    unsafe {
        io_uring
            .submission()
            .available()
            .push(entry.build().user_data(0x42))
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

    assert_eq!(buf, text);

    // unregister
    io_uring.unregister(unreg::Target::Buffer)?;

    Ok(())
}

#[test]
fn test_eventfd() -> anyhow::Result<()> {
    let mut io_uring = IoUring::new(1)?;
    let efd = eventfd(0, EfdFlags::EFD_NONBLOCK)?; // just test, we don't close

    // register eventfd
    io_uring.register(reg::Target::Event(efd))?;

    let mut buf = [0; 8];

    // just try
    let ret = nix::unistd::read(efd, &mut buf);
    assert_eq!(ret.unwrap_err(), nix::Error::Sys(nix::errno::Errno::EAGAIN));
    assert_eq!(u64::from_le_bytes(buf), 0);

    // nop
    unsafe {
        io_uring
            .submission()
            .available()
            .push(opcode::Nop::new().build().user_data(0x42))
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

    // read eventfd
    nix::unistd::read(efd, &mut buf)?;
    assert_eq!(u64::from_le_bytes(buf), 0x01);

    io_uring.unregister(unreg::Target::Event)?;

    Ok(())
}
