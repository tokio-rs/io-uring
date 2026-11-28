use crate::Test;
use io_uring::{cqueue, opcode, squeue, types, IoUring};

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
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        is_register_ring_fd_supported(ring)?;
    );

    println!("test register_ring_fd");

    let mut submitter = ring.submitter();
    match submitter.register_ring_fd() {
        Ok(()) => {}
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

    let mut probe = io_uring::Probe::new();
    submitter.register_probe(&mut probe)?;

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

fn is_register_ring_fd_supported<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
) -> anyhow::Result<bool> {
    let mut submitter = ring.submitter();

    match submitter.register_ring_fd() {
        Ok(()) => {
            submitter.unregister_ring_fd()?;
            Ok(true)
        }
        Err(e) if e.raw_os_error() == Some(libc::EINVAL) => Ok(false),
        Err(e) => Err(anyhow::anyhow!("register_ring_fd probe failed: {}", e)),
    }
}

pub fn test_register_file_alloc_range(test: &Test) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Socket::CODE);
    );

    println!("test register_file_alloc_range");

    let mut ring = IoUring::new(8)?;
    ring.submitter().register_files_sparse(16)?;

    // The range must lie inside the registered table.
    match ring.submitter().register_file_alloc_range(12, 8) {
        Err(e) if e.raw_os_error() == Some(libc::EINVAL) => {}
        other => {
            return Err(anyhow::anyhow!(
                "an out of range allocation window should be rejected: {:?}",
                other
            ));
        }
    }

    ring.submitter().register_file_alloc_range(4, 4)?;

    // Five sockets asked for an automatic slot: the first four fill the window, the fifth finds
    // no free slot even though the rest of the table is empty.
    for i in 0..5 {
        let socket = opcode::Socket::new(libc::AF_UNIX, libc::SOCK_STREAM, 0)
            .file_index(Some(types::DestinationSlot::auto_target()))
            .build()
            .user_data(i);
        unsafe {
            ring.submission().push(&socket).expect("queue is full");
        }
    }
    assert_eq!(ring.submit_and_wait(5)?, 5);

    let mut results = vec![0; 5];
    for cqe in ring.completion() {
        results[cqe.user_data() as usize] = cqe.result();
    }
    results.sort_unstable();
    assert_eq!(results, [-libc::ENFILE, 4, 5, 6, 7]);

    Ok(())
}

pub fn test_register_clock(test: &Test) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Timeout::CODE);
    );

    println!("test register_clock");

    let ring = IoUring::<squeue::Entry, cqueue::Entry>::new(2)?;
    let submitter = ring.submitter();

    // Kernels before 6.12 do not know the operation at all.
    match submitter.register_clock(libc::CLOCK_BOOTTIME) {
        Ok(()) => {}
        Err(e) if e.raw_os_error() == Some(libc::EINVAL) => return Ok(()),
        Err(e) => return Err(e.into()),
    }
    submitter.register_clock(libc::CLOCK_MONOTONIC)?;

    // Only the monotonic and boottime clocks can drive timeouts.
    let err = submitter.register_clock(libc::CLOCK_REALTIME).unwrap_err();
    assert_eq!(err.raw_os_error(), Some(libc::EINVAL));

    Ok(())
}
