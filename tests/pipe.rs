mod common;

use std::{ io, thread };
use std::time::{ Instant, Duration };
use std::os::unix::io::AsRawFd;
use io_uring::opcode::{ self, types };
use io_uring::IoUring;
use common::Fd;


#[test]
fn test_pipe_part_read() -> anyhow::Result<()> {
    let mut ring = IoUring::new(1)?;
    let (rp, wp) = nix::unistd::pipe()?;
    let (rp, wp) = (Fd(rp), Fd(wp));

    nix::unistd::write(wp.as_raw_fd(), b"hello")?;

    let mut buf = [0; 8];
    let mut bufs = [io::IoSliceMut::new(&mut buf)];
    let entry = {
        let rp = types::Target::Fd(rp.as_raw_fd());
        let bufs_ptr = bufs.as_mut_ptr() as *mut _;
        opcode::Readv::new(rp, bufs_ptr, 1)
    };

    unsafe {
        ring
            .submission()
            .available()
            .push(entry.build().user_data(0x42))
            .ok()
            .expect("queue is full");
    }
    ring.submit_and_wait(1)?;
    let entry = ring
        .completion()
        .available()
        .next()
        .expect("queue is empty");
    assert_eq!(entry.result(), 5);
    assert_eq!(entry.user_data(), 0x42);

    assert_eq!(&bufs[0][..5], b"hello");


    let entry = {
        let rp = types::Target::Fd(rp.as_raw_fd());
        let bufs_ptr = bufs.as_mut_ptr() as *mut _;
        opcode::Readv::new(rp, bufs_ptr, 1)
    };

    unsafe {
        ring
            .submission()
            .available()
            .push(entry.build().user_data(0x43))
            .ok()
            .expect("queue is full");
    }

    let h = thread::spawn(move || {
        thread::sleep(Duration::from_secs(1));
        nix::unistd::write(wp.as_raw_fd(), b"world")?;
        Ok(()) as anyhow::Result<()>
    });

    let now = Instant::now();
    ring.submit_and_wait(1)?;
    assert_eq!(now.elapsed().as_secs(), 1);

    let entry = ring
        .completion()
        .available()
        .next()
        .expect("queue is empty");
    assert_eq!(entry.result(), 5);
    assert_eq!(entry.user_data(), 0x43);

    assert_eq!(&buf[..5], b"world");

    h.join().unwrap()?;

    Ok(())
}
