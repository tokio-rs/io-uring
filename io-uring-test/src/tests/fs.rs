use crate::utils;
use crate::Test;
use io_uring::{cqueue, opcode, squeue, types, IoUring};
use std::ffi::CString;
use std::fs;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd};

pub fn test_file_write_read<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Write::CODE);
        test.probe.is_supported(opcode::Read::CODE);
    );

    println!("test file_write_read");

    let fd = tempfile::tempfile()?;
    let fd = types::Fd(fd.as_raw_fd());

    utils::write_read(ring, fd, fd)?;

    Ok(())
}

pub fn test_file_writev_readv<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Writev::CODE);
        test.probe.is_supported(opcode::Readv::CODE);
    );

    println!("test file_writev_readv");

    let fd = tempfile::tempfile()?;
    let fd = types::Fd(fd.as_raw_fd());

    utils::writev_readv(ring, fd, fd)?;

    Ok(())
}

pub fn test_file_fsync<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Fsync::CODE);
    );

    println!("test file_fsync");

    let mut fd = tempfile::tempfile()?;
    let n = fd.write(&[0x1])?;
    assert_eq!(n, 1);

    let fd = types::Fd(fd.as_raw_fd());

    let fsync_e = opcode::Fsync::new(fd);

    unsafe {
        ring.submission()
            .push(&fsync_e.build().user_data(0x03).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x03);
    assert_eq!(cqes[0].result(), 0);

    Ok(())
}

pub fn test_file_fsync_file_range<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::SyncFileRange::CODE);
    );

    println!("test file_fsync_file_range");

    let mut fd = tempfile::tempfile()?;
    let n = fd.write(&[0x2; 3 * 1024])?;
    assert_eq!(n, 3 * 1024);
    let n = fd.write(&[0x3; 1024])?;
    assert_eq!(n, 1024);

    let fd = types::Fd(fd.as_raw_fd());

    let fsync_e = opcode::SyncFileRange::new(fd, 1024).offset(3 * 1024);

    unsafe {
        ring.submission()
            .push(&fsync_e.build().user_data(0x04).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x04);
    assert_eq!(cqes[0].result(), 0);

    Ok(())
}

#[allow(deprecated)]
pub fn test_file_fallocate<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Fallocate::CODE);
    );

    println!("test file_fallocate");

    let fd = tempfile::tempfile()?;
    let fd = types::Fd(fd.as_raw_fd());

    let falloc_e = opcode::Fallocate::new(fd, 1024);

    unsafe {
        ring.submission()
            .push(&falloc_e.build().user_data(0x10).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x10);
    assert_eq!(cqes[0].result(), 0);

    Ok(())
}

pub fn test_file_fallocate64<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Fallocate64::CODE);
    );

    println!("test file_fallocate64");

    let fd = tempfile::tempfile()?;
    let fd = types::Fd(fd.as_raw_fd());

    let falloc_e = opcode::Fallocate64::new(fd, 1024);

    unsafe {
        ring.submission()
            .push(&falloc_e.build().user_data(0x20).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x20);
    assert_eq!(cqes[0].result(), 0);

    Ok(())
}

pub fn test_file_openat2<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::OpenAt2::CODE);
    );

    use tempfile::tempdir;

    println!("test file_openat2");

    let dir = tempdir()?;
    let dirfd = types::Fd(libc::AT_FDCWD);

    let path = dir.path().join("test-io-uring-openat2");
    let path = CString::new(path.as_os_str().as_bytes())?;

    let openhow = types::OpenHow::new().flags(libc::O_CREAT as _);
    let open_e = opcode::OpenAt2::new(dirfd, path.as_ptr(), &openhow);

    unsafe {
        ring.submission()
            .push(&open_e.build().user_data(0x11).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x11);
    assert!(cqes[0].result() > 0);

    let fd = unsafe { fs::File::from_raw_fd(cqes[0].result()) };

    assert!(fd.metadata()?.is_file());

    Ok(())
}

pub fn test_file_openat2_close_file_index<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    // Tests close too.

    require!(
        test;
        test.probe.is_supported(opcode::OpenAt2::CODE);
        test.probe.is_supported(opcode::Close::CODE);
        test.probe.is_supported(opcode::Socket::CODE); // to ensure fixed table support
    );

    // Cleanup all fixed files (if any), then reserve two slots.
    let _ = ring.submitter().unregister_files();
    ring.submitter().register_files_sparse(2).unwrap();

    use tempfile::tempdir;

    println!("test file_openat2_close_file_index");

    let dir = tempdir()?;
    let dirfd = types::Fd(libc::AT_FDCWD);

    // One more round than table size.
    for round in 0..3 {
        let path = dir.path().join(format!(
            "test-io-uring-openat2-file_index-a-round-{}",
            round
        ));
        let path = CString::new(path.as_os_str().as_bytes())?;

        let openhow = types::OpenHow::new().flags(libc::O_CREAT as _);

        let file_index = types::DestinationSlot::auto_target();

        let op = opcode::OpenAt2::new(dirfd, path.as_ptr(), &openhow);
        let op = op.file_index(Some(file_index));

        unsafe {
            ring.submission()
                .push(&op.build().user_data(0x11).into())
                .expect("queue is full");
        }

        ring.submit_and_wait(1)?;

        let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

        assert_eq!(cqes.len(), 1);
        assert_eq!(cqes[0].user_data(), 0x11);
        if round == 2 {
            assert!(cqes[0].result() < 0); // expect no room
        } else {
            assert_eq!(cqes[0].result(), round); // expect auto selection to go 0, then 1.
        }
    }

    // Drop two.
    for round in 0..2 {
        let op = opcode::Close::new(types::Fixed(round));

        unsafe {
            ring.submission()
                .push(&op.build().user_data(0x12).into())
                .expect("queue is full");
        }

        ring.submit_and_wait(1)?;

        let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

        assert_eq!(cqes.len(), 1);
        assert_eq!(cqes[0].user_data(), 0x12);
        assert_eq!(cqes[0].result(), 0); // successful close iff result is 0
    }

    // Redo the tests but with manual selection of the file_index value,
    // and reverse the order for good measure: so 2, 1, then 0.
    // Another difference: the sucessful result should be zero, not the fixed slot number since
    // we have not asked for an auto selection to be made for us.

    // One more round than table size.
    for round in (0..3).rev() {
        let path = dir.path().join(format!(
            "test-io-uring-openat2-file_index-b-round-{}",
            round
        ));
        let path = CString::new(path.as_os_str().as_bytes())?;

        let openhow = types::OpenHow::new().flags(libc::O_CREAT as _);

        let file_index = types::DestinationSlot::try_from_slot_target(round).unwrap();

        let op = opcode::OpenAt2::new(dirfd, path.as_ptr(), &openhow);
        let op = op.file_index(Some(file_index));

        unsafe {
            ring.submission()
                .push(&op.build().user_data(0x11).into())
                .expect("queue is full");
        }

        ring.submit_and_wait(1)?;

        let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

        assert_eq!(cqes.len(), 1);
        assert_eq!(cqes[0].user_data(), 0x11);
        if round == 2 {
            assert!(cqes[0].result() < 0); // expect 2 won't fit, even though it is being asked for first.
        } else {
            assert_eq!(cqes[0].result(), 0); // success iff zero
        }
    }

    // Drop two.
    for round in 0..2 {
        let op = opcode::Close::new(types::Fixed(round));

        unsafe {
            ring.submission()
                .push(&op.build().user_data(0x12).into())
                .expect("queue is full");
        }

        ring.submit_and_wait(1)?;

        let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

        assert_eq!(cqes.len(), 1);
        assert_eq!(cqes[0].user_data(), 0x12);
        assert_eq!(cqes[0].result(), 0); // successful close iff result is 0
    }
    // If the fixed-socket operation worked properly, this must not fail.
    ring.submitter().unregister_files().unwrap();

    Ok(())
}

// This is like the openat2 test of the same name, but uses openat instead.
pub fn test_file_openat_close_file_index<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    // Tests close too.

    require!(
        test;
        test.probe.is_supported(opcode::OpenAt::CODE);
        test.probe.is_supported(opcode::Close::CODE);
        test.probe.is_supported(opcode::Socket::CODE); // to ensure fixed table support
    );

    // Cleanup all fixed files (if any), then reserve two slots.
    let _ = ring.submitter().unregister_files();
    ring.submitter().register_files_sparse(2).unwrap();

    use tempfile::tempdir;

    println!("test file_openat_close_file_index");

    let dir = tempdir()?;
    let dirfd = types::Fd(libc::AT_FDCWD);

    // One more round than table size.
    for round in 0..3 {
        let path = dir
            .path()
            .join(format!("test-io-uring-openat-file_index-a-round-{}", round));
        let path = CString::new(path.as_os_str().as_bytes())?;

        let file_index = types::DestinationSlot::auto_target();

        let op = opcode::OpenAt::new(dirfd, path.as_ptr());
        let op = op.flags(libc::O_CREAT as _);
        let op = op.file_index(Some(file_index));

        unsafe {
            ring.submission()
                .push(&op.build().user_data(0x11).into())
                .expect("queue is full");
        }

        ring.submit_and_wait(1)?;

        let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

        assert_eq!(cqes.len(), 1);
        assert_eq!(cqes[0].user_data(), 0x11);
        if round == 2 {
            assert!(cqes[0].result() < 0); // expect no room
        } else {
            assert_eq!(cqes[0].result(), round); // expect auto selection to go 0, then 1.
        }
    }

    // Drop two.
    for round in 0..2 {
        let op = opcode::Close::new(types::Fixed(round));

        unsafe {
            ring.submission()
                .push(&op.build().user_data(0x12).into())
                .expect("queue is full");
        }

        ring.submit_and_wait(1)?;

        let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

        assert_eq!(cqes.len(), 1);
        assert_eq!(cqes[0].user_data(), 0x12);
        assert_eq!(cqes[0].result(), 0); // successful close iff result is 0
    }

    // Redo the tests but with manual selection of the file_index value,
    // and reverse the order for good measure: so 2, 1, then 0.
    // Another difference: the sucessful result should be zero, not the fixed slot number since
    // we have not asked for an auto selection to be made for us.

    // One more round than table size.
    for round in (0..3).rev() {
        let path = dir
            .path()
            .join(format!("test-io-uring-openat-file_index-b-round-{}", round));
        let path = CString::new(path.as_os_str().as_bytes())?;

        let file_index = types::DestinationSlot::try_from_slot_target(round).unwrap();

        let op = opcode::OpenAt::new(dirfd, path.as_ptr());
        let op = op.flags(libc::O_CREAT as _);
        let op = op.file_index(Some(file_index));

        unsafe {
            ring.submission()
                .push(&op.build().user_data(0x11).into())
                .expect("queue is full");
        }

        ring.submit_and_wait(1)?;

        let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

        assert_eq!(cqes.len(), 1);
        assert_eq!(cqes[0].user_data(), 0x11);
        if round == 2 {
            assert!(cqes[0].result() < 0); // expect 2 won't fit, even though it is being asked for first.
        } else {
            assert_eq!(cqes[0].result(), 0); // success iff zero
        }
    }

    // Drop two.
    for round in 0..2 {
        let op = opcode::Close::new(types::Fixed(round));

        unsafe {
            ring.submission()
                .push(&op.build().user_data(0x12).into())
                .expect("queue is full");
        }

        ring.submit_and_wait(1)?;

        let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

        assert_eq!(cqes.len(), 1);
        assert_eq!(cqes[0].user_data(), 0x12);
        assert_eq!(cqes[0].result(), 0); // successful close iff result is 0
    }
    // If the fixed-socket operation worked properly, this must not fail.
    ring.submitter().unregister_files().unwrap();

    Ok(())
}

pub fn test_file_close<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Close::CODE);
    );

    println!("test file_cloes");

    let fd = tempfile::tempfile()?;
    let fd = types::Fd(fd.into_raw_fd());

    let close_e = opcode::Close::new(fd);

    unsafe {
        ring.submission()
            .push(&close_e.build().user_data(0x12).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x12);
    assert_eq!(cqes[0].result(), 0);

    Ok(())
}

pub fn test_file_cur_pos<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Write::CODE);
        test.probe.is_supported(opcode::Read::CODE);
        ring.params().is_feature_rw_cur_pos();
    );

    println!("test file_cur_pos");

    let fd = tempfile::tempfile()?;
    let fd = types::Fd(fd.into_raw_fd());

    let text = b"The quick brown fox jumps over the lazy dog.";
    let mut output = vec![0; text.len()];

    let write_e = opcode::Write::new(fd, text.as_ptr(), 22)
        .offset(-1)
        .build()
        .user_data(0x01)
        .into();

    unsafe {
        ring.submission().push(&write_e).expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let write_e = opcode::Write::new(fd, unsafe { text.as_ptr().add(22) }, 22)
        .offset(-1)
        .build()
        .user_data(0x02)
        .into();

    unsafe {
        ring.submission().push(&write_e).expect("queue is full");
    }

    ring.submit_and_wait(2)?;

    let read_e = opcode::Read::new(fd, output.as_mut_ptr(), output.len() as _);

    unsafe {
        ring.submission()
            .push(&read_e.build().user_data(0x03).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(3)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 3);
    assert_eq!(cqes[0].user_data(), 0x01);
    assert_eq!(cqes[1].user_data(), 0x02);
    assert_eq!(cqes[2].user_data(), 0x03);
    assert_eq!(cqes[0].result(), 22);
    assert_eq!(cqes[1].result(), 22);
    assert_eq!(cqes[2].result(), text.len() as i32);

    assert_eq!(&output, text);

    Ok(())
}

/// Skip ci, because statx does not exist in old release.
#[cfg(not(feature = "ci"))]
pub fn test_statx<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Statx::CODE);
    );

    println!("test statx");

    let dir = tempfile::tempdir()?;
    let path = dir.path().join("test-io-uring-statx");
    let pathbuf = CString::new(path.as_os_str().as_bytes())?;
    fs::write(&path, "1")?;

    let mut statxbuf: libc::statx = unsafe { std::mem::zeroed() };

    let statx_e = opcode::Statx::new(
        types::Fd(libc::AT_FDCWD),
        pathbuf.as_ptr(),
        &mut statxbuf as *mut libc::statx as *mut _,
    )
    .mask(libc::STATX_ALL)
    .build()
    .user_data(0x99)
    .into();

    unsafe {
        ring.submission().push(&statx_e).expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x99);
    assert_eq!(cqes[0].result(), 0);

    // check
    let mut statxbuf2 = unsafe { std::mem::zeroed() };
    let ret = unsafe {
        libc::statx(
            libc::AT_FDCWD,
            pathbuf.as_ptr(),
            0,
            libc::STATX_ALL,
            &mut statxbuf2,
        )
    };

    assert_eq!(ret, 0);
    assert_eq!(statxbuf, statxbuf2);

    // statx fd
    let fd = fs::File::open(&path)?;
    let mut statxbuf3: libc::statx = unsafe { std::mem::zeroed() };

    let statx_e = opcode::Statx::new(
        types::Fd(fd.as_raw_fd()),
        b"\0".as_ptr().cast(),
        &mut statxbuf3 as *mut libc::statx as *mut _,
    )
    .flags(libc::AT_EMPTY_PATH)
    .mask(libc::STATX_ALL)
    .build()
    .user_data(0x9a)
    .into();

    unsafe {
        ring.submission().push(&statx_e).expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x9a);
    assert_eq!(cqes[0].result(), 0);

    assert_eq!(statxbuf3, statxbuf2);

    Ok(())
}

/// Skip ci, because direct IO does not work on qemu.
#[cfg(not(feature = "ci"))]
pub fn test_file_direct_write_read<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    use tempfile::TempDir;

    #[repr(align(4096))]
    struct AlignedBuffer([u8; 4096]);

    require!(
        test;
        test.probe.is_supported(opcode::Write::CODE);
        test.probe.is_supported(opcode::Read::CODE);
    );

    println!("test file_direct_write_read");

    let dir = TempDir::new_in(".")?;
    let fd = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_DIRECT)
        .open(dir.path().join("io-uring-test-file"))?;
    let fd = types::Fd(fd.as_raw_fd());

    // ok

    let input = Box::new(AlignedBuffer([0xf9; 4096]));
    let mut output = Box::new(AlignedBuffer([0x0; 4096]));

    let write_e = opcode::Write::new(fd, input.0.as_ptr(), input.0.len() as _);
    let read_e = opcode::Read::new(fd, output.0.as_mut_ptr(), output.0.len() as _);

    unsafe {
        ring.submission()
            .push(&write_e.build().user_data(0x01).into())
            .expect("queue is full");
    }

    assert_eq!(ring.submit_and_wait(1)?, 1);

    unsafe {
        ring.submission()
            .push(&read_e.build().user_data(0x02).into())
            .expect("queue is full");
    }

    assert_eq!(ring.submit_and_wait(2)?, 1);

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].user_data(), 0x01);
    assert_eq!(cqes[1].user_data(), 0x02);
    assert_eq!(cqes[0].result(), input.0.len() as i32);
    assert_eq!(cqes[1].result(), input.0.len() as i32);

    assert_eq!(input.0[..], output.0[..]);
    assert_eq!(input.0[0], 0xf9);

    // fail

    let mut buf = vec![0; 4097];

    let read_e = opcode::Read::new(fd, buf[1..].as_mut_ptr(), buf[1..].len() as _);

    unsafe {
        ring.submission()
            .push(&read_e.build().user_data(0x03).into())
            .expect("queue is full");
    }

    assert_eq!(ring.submit_and_wait(1)?, 1);

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x03);
    assert_eq!(cqes[0].result(), -libc::EINVAL);

    Ok(())
}

pub fn test_file_splice<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    use std::io::Read;

    require!(
        test;
        test.probe.is_supported(opcode::Splice::CODE);
    );

    println!("test file_splice");

    let dir = tempfile::TempDir::new_in(".")?;
    let dir = dir.path();

    let input = &[0x9f; 1024];

    let (pipe_in, mut pipe_out) = {
        let mut pipes = [0, 0];
        let ret = unsafe { libc::pipe(pipes.as_mut_ptr()) };
        assert_eq!(ret, 0);
        let pipe_out = unsafe { fs::File::from_raw_fd(pipes[0]) };
        let pipe_in = unsafe { fs::File::from_raw_fd(pipes[1]) };
        (pipe_in, pipe_out)
    };

    fs::write(dir.join("io-uring-test-file-input"), input)?;
    let fd = fs::File::open(dir.join("io-uring-test-file-input"))?;

    let splice_e = opcode::Splice::new(
        types::Fd(fd.as_raw_fd()),
        0,
        types::Fd(pipe_in.as_raw_fd()),
        -1,
        1024,
    );

    unsafe {
        ring.submission()
            .push(&splice_e.build().user_data(0x33).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x33);
    assert_eq!(cqes[0].result(), 1024);

    let mut output = [0; 1024];
    pipe_out.read_exact(&mut output)?;

    assert_eq!(input, &output[..]);

    Ok(())
}
