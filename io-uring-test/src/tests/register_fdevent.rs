use crate::Test;
use io_uring::{cqueue, opcode, squeue, IoUring};
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::thread;
use std::time::Duration;

/// Create a nonblocking eventfd and register it with `ring`.
fn register_fdevent<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
) -> anyhow::Result<File> {
    let fd = unsafe {
        let fd = libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK);

        if fd == -1 {
            return Err(io::Error::last_os_error().into());
        }

        File::from_raw_fd(fd)
    };

    ring.submitter().register_eventfd(fd.as_raw_fd())?;

    Ok(fd)
}

/// Submit a `Nop` and wait for its completion.
fn submit_nop<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
) -> anyhow::Result<()> {
    let entry = opcode::Nop::new().build().into();

    unsafe {
        ring.submission().push(&entry).expect("queue is full");
    }

    ring.submit_and_wait(1)?;
    assert_eq!(ring.completion().count(), 1);

    Ok(())
}

/// Whether the eventfd's counter became readable
fn eventfd_notified(fd: &mut File) -> anyhow::Result<bool> {
    thread::sleep(Duration::from_millis(200));

    let mut buf = [0u8; 8];
    match fd.read(&mut buf) {
        Ok(8) => Ok(true),
        Ok(n) => panic!("short read from eventfd: {} bytes", n),
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(false),
        Err(e) => Err(e.into()),
    }
}

pub fn test_register_eventfd<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
    );

    println!("test register_eventfd");

    let mut fd = register_fdevent(ring)?;

    // register, get notification
    submit_nop(ring)?;
    assert!(
        eventfd_notified(&mut fd)?,
        "expected a notification while registered"
    );

    // unregister, no notification
    ring.submitter().unregister_eventfd()?;
    submit_nop(ring)?;
    assert!(
        !eventfd_notified(&mut fd)?,
        "did not expect a notification after unregistering"
    );

    Ok(())
}

pub fn test_register_eventfd_disable<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
    );

    println!("test register_eventfd_disable");

    let mut fd = register_fdevent(ring)?;

    // register, get notification
    submit_nop(ring)?;
    assert!(
        eventfd_notified(&mut fd)?,
        "expected a notification while enabled"
    );

    // disable, no notification
    ring.completion().disable_eventfd();
    submit_nop(ring)?;
    assert!(
        !eventfd_notified(&mut fd)?,
        "did not expect a notification while disabled"
    );

    // enable, notification
    ring.completion().enable_eventfd();
    submit_nop(ring)?;
    assert!(
        eventfd_notified(&mut fd)?,
        "expected a notification after re-enabling"
    );

    ring.submitter().unregister_eventfd()?;
    submit_nop(ring)?;
    assert!(
        !eventfd_notified(&mut fd)?,
        "did not expect a notification after unregistering"
    );

    Ok(())
}
