use std::io;
use std::os::unix::io::AsRawFd;
use tempfile::tempfile;
use linux_io_uring::{ opcode, reg, squeue, SetupFlag, Builder };


#[ignore]
#[test]
fn test_fs_sqpoll() -> anyhow::Result<()> {
    assert_eq!(unsafe { libc::geteuid() }, 0);

    let mut io_uring = Builder::new(4)
        .setup_flags(SetupFlag::SQPOLL)
        .build()?;
    let fd = tempfile()?;

    let text = b"hello world!";
    let mut buf = vec![0; 4];
    let mut buf2 = vec![0; 4];
    let mut buf3 = vec![0; 4];
    let bufs = [io::IoSlice::new(text)];
    let mut bufs2 = [io::IoSliceMut::new(&mut buf)];
    let mut bufs3 = [io::IoSliceMut::new(&mut buf2), io::IoSliceMut::new(&mut buf3)];

    io_uring.register(reg::Target::File(&[fd.as_raw_fd()]))?;

    let write_entry =
        opcode::Writev::new(opcode::Target::Fixed(0), bufs.as_ptr() as *const _, 1)
        .build();
    let read_entry =
        opcode::Readv::new(opcode::Target::Fixed(0), bufs2.as_mut_ptr() as *mut _, 1)
        .build()
        .flags(squeue::Flag::IO_DRAIN);
    let read_entry2 =
        opcode::Readv::new(opcode::Target::Fixed(0), bufs3.as_mut_ptr() as *mut _, 2)
        .offset(4)
        .build();

    unsafe {
        io_uring
            .submission()
            .available()
            .push(write_entry)
            .ok()
            .expect("queue is full");
    }

    assert!(io_uring.submission().need_wakeup());
    io_uring.submit_and_wait(1)?;

    unsafe {
        let mut sq = io_uring
            .submission()
            .available();
        sq.push(read_entry)
            .ok()
            .expect("queue is full");
        sq.push(read_entry2)
            .ok()
            .expect("queue is full");
    }

    // fast poll
    while !io_uring.submission().is_empty() {
        std::thread::yield_now();
    }

    io_uring.submit_and_wait(2)?;

    assert_eq!(io_uring.completion().len(), 3);

    io_uring
        .completion()
        .available()
        .for_each(drop);

    assert_eq!(&text[..4], &buf[..]);
    assert_eq!(&text[4..][..4], &buf2[..]);
    assert_eq!(&text[4..][4..][..4], &buf3[..]);

    Ok(())
}
