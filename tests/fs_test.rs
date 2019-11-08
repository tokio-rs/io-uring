use linux_io_uring::{opcode, IoUring};
use std::io::{self, Read};
use std::os::unix::io::AsRawFd;
use tempfile::tempfile;

#[test]
fn test_fs() -> anyhow::Result<()> {
    let text = b"hello world!";

    let mut io_uring = IoUring::new(1)?;
    let mut fd = tempfile()?;

    // writev submit
    let entry = opcode::Writev::new(
        opcode::Target::Fd(fd.as_raw_fd()),
        [io::IoSlice::new(text)].as_ptr() as *const _,
        1,
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

    // writev complete
    let entry = io_uring
        .completion()
        .available()
        .next()
        .expect("queue is empty");
    assert_eq!(entry.user_data(), 0x42);

    let mut buf = Vec::new();
    fd.read_to_end(&mut buf)?;
    assert_eq!(buf, text);

    // readv submit
    let mut buf2 = [0; 12];
    assert_eq!(buf2.len(), text.len());

    let entry = opcode::Readv::new(
        opcode::Target::Fd(fd.as_raw_fd()),
        [io::IoSliceMut::new(&mut buf2)].as_mut_ptr() as *mut _,
        1,
    );

    unsafe {
        io_uring
            .submission()
            .available()
            .push(entry.build().user_data(0x43))
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

    assert_eq!(&buf2, text);

    Ok(())
}
