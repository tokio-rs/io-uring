use std::io::{ self, Read };
use std::os::unix::io::AsRawFd;
use tempfile::tempfile;
use linux_io_uring::{ squeue, opcode, IoUring };


#[test]
fn test_fs() -> io::Result<()> {
    const TEXT: &[u8] = b"hello world!";

    let mut io_uring = IoUring::new(1)?;

    let mut fd = tempfile()?;

    // writev submit
    let mut entry = opcode::Writev::default();
    entry.fd = opcode::Target::Fd(fd.as_raw_fd());
    entry.iovec = [io::IoSlice::new(TEXT)].as_ptr() as *const _;
    entry.len = 1;

    unsafe {
        io_uring
            .submission()
            .available()
            .push(squeue::Entry::from(entry).user_data(0x42))
            .map_err(drop)
            .expect("queue is full");
    }

    io_uring.submit_and_wait(1)?;

    // writev complete
    let entry = io_uring
        .completion()
        .available()
        .next()
        .expect("queue is empty");
    assert_eq!(entry.user_data(), 0x42);

    let mut buf = Vec::new();
    fd.read_to_end(&mut buf)?;
    assert_eq!(buf, TEXT);

    // readv submit
    let mut buf2 = vec![0; TEXT.len()];

    let mut entry = opcode::Readv::default();
    entry.fd = opcode::Target::Fd(fd.as_raw_fd());
    entry.iovec = [io::IoSliceMut::new(&mut buf2)].as_mut_ptr() as *mut _;
    entry.len = 1;

    unsafe {
        io_uring
            .submission()
            .available()
            .push(squeue::Entry::from(entry).user_data(0x43))
            .map_err(drop)
            .expect("queue is full");
    }

    io_uring.submit_and_wait(1)?;

    // readv complete
    let entry = io_uring
        .completion()
        .available()
        .next()
        .expect("queue is empty");
    assert_eq!(entry.user_data(), 0x43);

    assert_nq!(buf2, TEXT);

    Ok(())
}
