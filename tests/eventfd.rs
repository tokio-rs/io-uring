mod common;

use std::{ io, thread };
use std::sync::Arc;
use std::convert::TryInto;
use std::time::{ Duration, Instant };
use std::os::unix::io::AsRawFd;
use nix::sys::eventfd::*;
use linux_io_uring::opcode::{ self, types };
use linux_io_uring::IoUring;
use common::Fd;


#[test]
fn test_eventfd() -> anyhow::Result<()> {
    let mut io_uring = IoUring::new(1)?;
    let efd: Arc<Fd> = eventfd(0, EfdFlags::EFD_CLOEXEC)?
        .try_into()
        .map(Arc::new)
        .map_err(|_| anyhow::format_err!("invalid fd"))?;

    let mut buf = [0; 8];
    let mut bufs = [io::IoSliceMut::new(&mut buf)];
    let entry = {
        let efd = types::Target::Fd(efd.as_raw_fd());
        let bufs_ptr = bufs.as_mut_ptr() as *mut _;
        opcode::Readv::new(efd, bufs_ptr, 1)
    };

    // push read eventfd
    unsafe {
        io_uring
            .submission()
            .available()
            .push(entry.build().user_data(0x42))
            .map_err(drop)
            .expect("queue is full");
    }
    io_uring.submit()?;

    let now = Instant::now();
    let efd2 = efd.clone();
    let handle = thread::spawn(move || {
        thread::sleep(Duration::from_secs(1));

        // wake eventfd
        nix::unistd::write(efd2.as_raw_fd(), &0x1u64.to_le_bytes())?;
        nix::unistd::write(efd2.as_raw_fd(), &0x1u64.to_le_bytes())?;
        Ok(()) as anyhow::Result<()>
    });

    // wait
    io_uring.submit_and_wait(1)?;
    let t = now.elapsed();
    assert_eq!(t.as_secs(), 1);

    let entry = io_uring
        .completion()
        .available()
        .next()
        .expect("queue is empty");
    assert_eq!(u64::from_le_bytes(buf), 2);
    assert_eq!(entry.user_data(), 0x42);

    handle.join().unwrap()?;

    Ok(())
}
