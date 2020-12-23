use crate::helper;
use io_uring::opcode::{self, types};
use io_uring::IoUring;
use std::fs;
use std::io::Write;
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd};

pub fn test_file_write_read(ring: &mut IoUring) -> anyhow::Result<()> {
    println!("test file_write_read");

    let fd = tempfile::tempfile()?;
    let fd = types::Fd(fd.as_raw_fd());

    helper::write_read(ring, fd, fd)?;

    Ok(())
}

pub fn test_file_writev_readv(ring: &mut IoUring) -> anyhow::Result<()> {
    println!("test file_writev_readv");

    let fd = tempfile::tempfile()?;
    let fd = types::Fd(fd.as_raw_fd());

    helper::writev_readv(ring, fd, fd)?;

    Ok(())
}

pub fn test_file_fsync(ring: &mut IoUring) -> anyhow::Result<()> {
    println!("test file_fsync");

    let mut fd = tempfile::tempfile()?;
    let n = fd.write(&[0x1])?;
    assert_eq!(n, 1);

    let fd = types::Fd(fd.as_raw_fd());

    let fsync_e = opcode::Fsync::new(fd);

    unsafe {
        let mut queue = ring.submission().available();
        queue
            .push(fsync_e.build().user_data(0x03))
            .ok()
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes = ring.completion().available().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x03);
    assert_eq!(cqes[0].result(), 0);

    Ok(())
}

pub fn test_file_fsync_file_range(ring: &mut IoUring) -> anyhow::Result<()> {
    println!("test file_fsync_file_range");

    let mut fd = tempfile::tempfile()?;
    let n = fd.write(&[0x2; 3 * 1024])?;
    assert_eq!(n, 3 * 1024);
    let n = fd.write(&[0x3; 1024])?;
    assert_eq!(n, 1024);

    let fd = types::Fd(fd.as_raw_fd());

    let fsync_e = opcode::SyncFileRange::new(fd, 1024).offset(3 * 1024);

    unsafe {
        let mut queue = ring.submission().available();
        queue
            .push(fsync_e.build().user_data(0x04))
            .ok()
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes = ring.completion().available().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x04);
    assert_eq!(cqes[0].result(), 0);

    Ok(())
}

pub fn test_file_fallocate(ring: &mut IoUring) -> anyhow::Result<()> {
    println!("test file_fallocate");

    let fd = tempfile::tempfile()?;
    let fd = types::Fd(fd.as_raw_fd());

    let falloc_e = opcode::Fallocate::new(fd, 1024);

    unsafe {
        ring.submission()
            .available()
            .push(falloc_e.build().user_data(0x10))
            .ok()
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes = ring.completion().available().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x10);
    assert_eq!(cqes[0].result(), 0);

    Ok(())
}

pub fn test_file_openat2(ring: &mut IoUring) -> anyhow::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use tempfile::tempdir;

    println!("test file_openat2");

    let dir = tempdir()?;
    let dirfd = types::Fd(libc::AT_FDCWD);

    let path = dir.path().join("test-io-uring-openat2");
    let path = CString::new(path.as_os_str().as_bytes())?;

    let openhow = types::OpenHow::new().flags(libc::O_CREAT as _);
    let open_e = opcode::Openat2::new(dirfd, path.as_ptr(), &openhow);

    unsafe {
        ring.submission()
            .available()
            .push(open_e.build().user_data(0x11))
            .ok()
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes = ring.completion().available().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x11);
    assert!(cqes[0].result() > 0);

    let fd = unsafe { fs::File::from_raw_fd(cqes[0].result()) };

    assert!(fd.metadata()?.is_file());

    Ok(())
}

pub fn test_file_close(ring: &mut IoUring) -> anyhow::Result<()> {
    println!("test file_cloes");

    let fd = tempfile::tempfile()?;
    let fd = types::Fd(fd.into_raw_fd());

    let close_e = opcode::Close::new(fd);

    unsafe {
        ring.submission()
            .available()
            .push(close_e.build().user_data(0x12))
            .ok()
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes = ring.completion().available().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x12);
    assert_eq!(cqes[0].result(), 0);

    Ok(())
}
