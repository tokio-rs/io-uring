mod common;

use std::io;
use std::os::unix::io::AsRawFd;
use io_uring::opcode::{ self, types };
use io_uring::IoUring;
use common::Fd;


#[test]
fn test_cancel_nop() -> anyhow::Result<()> {
    let mut ring = IoUring::new(4)?;

    let nop_e = opcode::Nop::new()
        .build()
        .user_data(0x01);
    let cancel_e = opcode::AsyncCancel::new(0x01)
        .build()
        .user_data(0x02);

    unsafe {
        let mut sq = ring.submission().available();
        sq.push(nop_e)
            .ok()
            .expect("squeue is full");
        sq.push(cancel_e)
            .ok()
            .expect("squeue is full");
    }

    ring.submit_and_wait(2)?;

    let mut cqes = ring
        .completion()
        .available()
        .collect::<Vec<_>>();
    cqes.sort_by_key(|cqe| cqe.user_data());

    assert_eq!(cqes[0].user_data(), 0x01);
    assert_eq!(cqes[0].result(), 0);
    assert_eq!(cqes[1].user_data(), 0x02);
    assert_eq!(cqes[1].result(), -libc::ENOENT);

    Ok(())
}

#[test]
fn test_cancel_pipe() -> anyhow::Result<()> {
    let mut ring = IoUring::new(4)?;
    let (rp, wp) = nix::unistd::pipe()?;
    let (rp, _wp) = (Fd(rp), Fd(wp));

    let mut buf = [0; 8];
    let mut bufs = [io::IoSliceMut::new(&mut buf)];
    let readv_e = {
        let rp = types::Target::Fd(rp.as_raw_fd());
        let bufs_ptr = bufs.as_mut_ptr() as *mut _;
        opcode::Readv::new(rp, bufs_ptr, 1)
    }
        .build()
        .user_data(0x01);

    let cancel_e = opcode::AsyncCancel::new(0x01)
        .build()
        .user_data(0x02);

    unsafe {
        ring
            .submission()
            .available()
            .push(readv_e)
            .ok()
            .expect("queue is full");
    }

    ring.submit()?;

    unsafe {
        ring
            .submission()
            .available()
            .push(cancel_e)
            .ok()
            .expect("queue is full");
    }

    ring.submit_and_wait(2)?;

    let mut cqes = ring
        .completion()
        .available()
        .collect::<Vec<_>>();
    cqes.sort_by_key(|cqe| cqe.user_data());

    assert_eq!(cqes[0].user_data(), 0x01);
    assert_eq!(cqes[0].result(), -libc::ECANCELED);
    assert_eq!(cqes[1].user_data(), 0x02);
    assert_eq!(cqes[1].result(), 0);

    Ok(())
}
