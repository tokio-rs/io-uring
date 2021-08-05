use io_uring::{opcode, types, IoUring};
use std::io::IoSliceMut;
use std::os::unix::io::AsRawFd;
use std::{fs, io};

fn main() -> io::Result<()> {
    let mut ring = IoUring::new(8)?;

    let fd = fs::File::open("README.md")?;

    let mut buf1 = [1; 4];
    let mut buf2 = [2; 8];
    let bufs = &mut [IoSliceMut::new(&mut buf1), IoSliceMut::new(&mut buf2)][..];

    let readv_e = opcode::Readv::new(types::Fd(fd.as_raw_fd()), bufs)
        .build()
        .user_data(0x42);

    // Note that the developer needs to ensure
    // that the entry pushed into submission queue is valid (e.g. fd, buffer).
    unsafe {
        ring.submission()
            .push(&readv_e)
            .expect("submission queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe = ring.completion().next().expect("completion queue is empty");

    assert_eq!(cqe.user_data(), 0x42);
    assert!(cqe.result() >= 0, "read error: {}", cqe.result());
    assert_eq!(&buf1[..], b"# Li");

    Ok(())
}
