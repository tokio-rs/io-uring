mod common;

use std::{ io, mem };
use std::os::unix::io::AsRawFd;
use tempfile::tempfile;
use nix::sys::eventfd::*;
use linux_io_uring::opcode::{ self, types };
use linux_io_uring::{ reg, unreg, IoUring };
use common::{ Fd, do_write_read };


#[test]
fn test_fixed_file() -> anyhow::Result<()> {
    let mut ring = IoUring::new(2)?;
    let fd = tempfile()?;

    // register fd
    unsafe {
        ring.register(reg::Target::Files(&[fd.as_raw_fd()]))?;
    }

    let fd = types::Target::Fixed(0);

    do_write_read(&mut ring, fd)?;

    // unregister
    ring.unregister(unreg::Target::Files)?;

    Ok(())
}

#[test]
fn test_fixed_buffer() -> anyhow::Result<()> {
    let text = b"hello world!";

    let mut ring = IoUring::new(1)?;
    let (rp, wp) = nix::unistd::pipe()?;
    let (rp, wp) = (Fd(rp), Fd(wp));

    let mut buf = vec![0; text.len()];

    // register buffer
    unsafe {
        let buf = io::IoSliceMut::new(&mut buf);
        ring.register(reg::Target::Buffers(&[mem::transmute(buf)]))?;
    }

    nix::unistd::write(wp.as_raw_fd(), text)?;

    // read fixed
    let entry = opcode::ReadFixed::new(
        types::Target::Fd(rp.as_raw_fd()),
        buf.as_mut_ptr(),
        buf.len() as _,
        0
    );

    unsafe {
        ring
            .submission()
            .available()
            .push(entry.build().user_data(0x42))
            .map_err(drop)
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;
    let entry = ring
        .completion()
        .available()
        .next()
        .expect("queue is empty");
    assert_eq!(entry.user_data(), 0x42);

    assert_eq!(buf, text);

    // unregister
    ring.unregister(unreg::Target::Buffers)?;

    Ok(())
}


#[test]
fn test_reg_eventfd() -> anyhow::Result<()> {
    let mut ring = IoUring::new(1)?;
    let efd = eventfd(0, EfdFlags::EFD_CLOEXEC | EfdFlags::EFD_NONBLOCK)?;

    // register eventfd
    unsafe {
        ring.register(reg::Target::EventFd(efd))?;
    }

    let mut buf = [0; 8];

    // just try
    let ret = nix::unistd::read(efd, &mut buf);
    assert_eq!(ret.unwrap_err(), nix::Error::Sys(nix::errno::Errno::EAGAIN));
    assert_eq!(u64::from_le_bytes(buf), 0);

    // nop
    unsafe {
        ring
            .submission()
            .available()
            .push(opcode::Nop::new().build().user_data(0x42))
            .map_err(drop)
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;
    let entry = ring
        .completion()
        .available()
        .next()
        .expect("queue is empty");
    assert_eq!(entry.user_data(), 0x42);

    // read eventfd
    nix::unistd::read(efd, &mut buf)?;
    assert_eq!(u64::from_le_bytes(buf), 0x01);

    ring.unregister(unreg::Target::EventFd)?;

    Ok(())
}
