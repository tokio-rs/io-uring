mod common;

use std::os::unix::io::AsRawFd;
use tempfile::tempfile;
use io_uring::opcode::types;
use io_uring::IoUring;
use common::do_write_read;


#[test]
fn test_file() -> anyhow::Result<()> {
    let mut ring = IoUring::new(2)?;

    let fd = tempfile()?;
    let fd = types::Target::Fd(fd.as_raw_fd());

    do_write_read(&mut ring, fd)?;

    Ok(())
}

#[cfg(feature = "concurrent")]
#[test]
fn test_conc_openat2() -> anyhow::Result<()> {
    use std::{ fs, thread };
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::tempdir;
    use io_uring::opcode;

    let ring = Arc::new(IoUring::new(2)?.concurrent());
    let ring2 = ring.clone();

    let dir = Arc::new(tempdir()?);
    let _dir = dir.clone();

    fs::create_dir_all(dir.path())?;

    thread::spawn(move || {
        thread::sleep(Duration::from_secs(1));

        let dirfd = libc::AT_FDCWD;

        let path = dir.path()
            .join("test-io-uring-openat2");
        let path = CString::new(path.as_os_str().as_bytes()).unwrap();

        let openhow = types::OpenHow::new()
            .flags(libc::O_CREAT as _);

        let entry = opcode::Openat2::new(dirfd, path.as_ptr(), &openhow)
            .build()
            .user_data(0x42);

        unsafe {
            ring2
                .submission()
                .push(entry)
                .ok()
                .expect("squeue is full");
        }

        ring2.submit().unwrap();
    });

    ring.submit_and_wait(1)?;

    let cqe = ring.completion()
        .pop()
        .expect("cqueue is empty");

    assert_eq!(cqe.user_data(), 0x42);
    assert!(cqe.result() > 0);

    Ok(())
}
