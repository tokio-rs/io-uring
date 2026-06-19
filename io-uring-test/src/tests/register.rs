use crate::Test;
use io_uring::{cqueue, opcode, squeue, IoUring};

pub fn test_register_files_sparse<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    // register_files_sparse was introduced in kernel 5.19, as was the opcode for UringCmd16.
    // So require the UringCmd16 to avoid running this test on earlier kernels.
    require!(
        test;
        test.probe.is_supported(opcode::UringCmd16::CODE);
    );

    println!("test register_files_sparse");

    ring.submitter().register_files_sparse(4)?;

    // See that same call again, with any value, will fail because a direct table cannot be built
    // over an existing one.

    if let Ok(()) = ring.submitter().register_files_sparse(3) {
        return Err(anyhow::anyhow!(
            "register_files_sparse should not have succeeded twice in a row"
        ));
    }

    // See that the direct table can be removed.

    if let Err(e) = ring.submitter().unregister_files() {
        return Err(anyhow::anyhow!("unrgister_files failed: {}", e));
    }

    // See that a second attempt to remove the direct table would fail.

    if let Ok(()) = ring.submitter().unregister_files() {
        return Err(anyhow::anyhow!(
            "unrgister_files should not have succeeded twice in a row"
        ));
    }

    // See that a new, large, direct table can be created.
    // If it fails with EMFILE, print the ulimit command for changing this.

    if let Err(e) = ring.submitter().register_files_sparse(10_000) {
        if let Some(raw_os_err) = e.raw_os_error() {
            if raw_os_err == libc::EMFILE {
                println!(
                    "could not open 10,000 file descriptors, try `ulimit -Sn 11000` in the shell"
                );
                return Ok(());
            } else {
                return Err(anyhow::anyhow!("register_files_sparse should have succeeded after the previous one was removed: {}", e));
            }
        } else {
            return Err(anyhow::anyhow!("register_files_sparse should have succeeded after the previous one was removed: {}", e));
        }
    }

    // And removed.

    if let Err(e) = ring.submitter().unregister_files() {
        return Err(anyhow::anyhow!(
            "unrgister_files failed, odd since the one could be unregistered earlier: {}",
            e
        ));
    }

    Ok(())
}

pub fn test_register_ring_fd<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    _test: &Test,
) -> anyhow::Result<()> {
    println!("test register_ring_fd");

    // register_ring_fd was introduced in kernel 5.18. Skip the test if it is not supported.
    let mut submitter = ring.submitter();
    match submitter.register_ring_fd() {
        Ok(()) => {}
        Err(e) if e.raw_os_error() == Some(libc::EINVAL) => {
            println!("register_ring_fd not supported, skipping");
            return Ok(());
        }
        Err(e) => return Err(anyhow::anyhow!("register_ring_fd failed: {}", e)),
    }

    match submitter.register_ring_fd() {
        Err(e) if e.raw_os_error() == Some(libc::EEXIST) => {}
        Ok(()) => {
            return Err(anyhow::anyhow!(
                "register_ring_fd should not have succeeded twice on the same submitter"
            ));
        }
        Err(e) => {
            return Err(anyhow::anyhow!(
                "register_ring_fd should have failed with EEXIST on the same submitter: {}",
                e
            ));
        }
    }

    let mut other_submitter = ring.submitter();
    match other_submitter.unregister_ring_fd() {
        Err(e) if e.raw_os_error() == Some(libc::EINVAL) => {}
        Ok(()) => {
            return Err(anyhow::anyhow!(
                "unregister_ring_fd should be scoped to the registering submitter"
            ));
        }
        Err(e) => {
            return Err(anyhow::anyhow!(
                "unregister_ring_fd should have failed with EINVAL on an unregistered submitter: {}",
                e
            ));
        }
    }
    other_submitter.register_ring_fd()?;
    match other_submitter.register_ring_fd() {
        Err(e) if e.raw_os_error() == Some(libc::EEXIST) => {}
        Ok(()) => {
            return Err(anyhow::anyhow!(
                "register_ring_fd should not have succeeded twice on another submitter"
            ));
        }
        Err(e) => {
            return Err(anyhow::anyhow!(
                "register_ring_fd should have failed with EEXIST on another submitter: {}",
                e
            ));
        }
    }
    other_submitter.unregister_ring_fd()?;

    submitter.unregister_ring_fd()?;

    let (mut submitter, mut sq, mut cq) = ring.split();
    submitter.register_ring_fd()?;

    // Submitting now transparently goes through the registered ring index.
    let nop = opcode::Nop::new().build().user_data(0x42).into();
    unsafe {
        sq.push(&nop).expect("submission queue is full");
    }
    sq.sync();
    submitter.submit_and_wait(1)?;
    cq.sync();
    let cqes: Vec<cqueue::Entry> = cq.by_ref().map(Into::into).collect();
    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x42);
    assert_eq!(cqes[0].result(), 0);

    submitter.unregister_ring_fd()?;

    match submitter.unregister_ring_fd() {
        Err(e) if e.raw_os_error() == Some(libc::EINVAL) => {}
        Ok(()) => {
            return Err(anyhow::anyhow!(
                "unregister_ring_fd should not have succeeded twice in a row"
            ));
        }
        Err(e) => {
            return Err(anyhow::anyhow!(
                "unregister_ring_fd should have failed with EINVAL after unregistering: {}",
                e
            ));
        }
    }

    // Submitting still works after unregistering, now via the raw file descriptor.
    let nop = opcode::Nop::new().build().user_data(0x43).into();
    unsafe {
        sq.push(&nop).expect("submission queue is full");
    }
    sq.sync();
    submitter.submit_and_wait(1)?;
    cq.sync();
    let cqes: Vec<cqueue::Entry> = cq.map(Into::into).collect();
    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x43);
    assert_eq!(cqes[0].result(), 0);

    Ok(())
}
