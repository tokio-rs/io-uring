use io_uring::{ opcode, IoUring };


#[test]
fn test_basic() -> anyhow::Result<()> {
    let mut ring = IoUring::new(4)?;

    // nop
    let entry = opcode::Nop::new()
        .build()
        .user_data(0x42);

    unsafe {
        ring.submission()
            .available()
            .push(entry)
            .ok()
            .expect("squeue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe = ring.completion()
        .available()
        .next()
        .expect("cqueue is empty");

    assert_eq!(cqe.user_data(), 0x42);

    Ok(())
}

#[test]
fn test_sq_full() -> anyhow::Result<()> {
    let mut ring = IoUring::new(4)?;

    for _ in 0..4 {
        unsafe {
            ring.submission()
                .available()
                .push(opcode::Nop::new().build())
                .ok()
                .expect("squeue is full");
        }
    }

    unsafe {
        let ret = ring.submission()
            .available()
            .push(opcode::Nop::new().build());

        assert!(ret.is_err());
    }

    assert!(ring.submission().is_full());
    assert_eq!(ring.submission().len(), 4);

    assert!(ring.submission().available().is_full());
    assert_eq!(ring.submission().available().len(), 4);

    Ok(())
}

#[cfg(feature = "unstable")]
#[test]
fn test_cq_overflow() -> anyhow::Result<()> {
    let mut ring = IoUring::new(4)?;

    // fill cq
    for _ in 0..2 {
        for _ in 0..4 {
            unsafe {
                ring.submission()
                    .available()
                    .push(opcode::Nop::new().build())
                    .ok()
                    .expect("squeue is full");
            }
        }

        ring.submit()?;
    }

    assert_eq!(ring.completion().len(), 8);
    assert!(ring.completion().is_full());

    if ring.params().is_feature_nodrop() {
        // 5.5+ kernel

        // overflow cq
        for _ in 0..4 {
            unsafe {
                ring.submission()
                    .available()
                    .push(opcode::Nop::new().build())
                    .ok()
                    .expect("squeue is full");
            }
        }

        ring.submit()?;

        assert_eq!(ring.completion().len(), 8);
        assert!(ring.completion().is_full());
        assert_eq!(ring.completion().overflow(), 0);

        // cq busy
        unsafe {
            ring.submission()
                .available()
                .push(opcode::Nop::new().build())
                .ok()
                .expect("squeue is full");
        }

        let err = ring.submit().unwrap_err();
        assert_eq!(err.raw_os_error(), Some(libc::EBUSY));
        assert_eq!(ring.completion().len(), 8);
        assert!(ring.completion().is_full());
        assert_eq!(ring.completion().overflow(), 0);

        // release cq
        ring.completion().available().take(4).for_each(drop);
        assert_eq!(ring.completion().len(), 4);

        unsafe {
            ring.submission()
                .available()
                .push(opcode::Nop::new().build())
                .ok()
                .expect("squeue is full");
        }

        ring.submit()?;

        // fill again
        assert_eq!(ring.completion().len(), 8);
    } else {
        // 5.4 kernel

        // overflow cq
        for _ in 0..4 {
            unsafe {
                ring.submission()
                    .available()
                    .push(opcode::Nop::new().build())
                    .ok()
                    .expect("squeue is full");
            }
        }

        ring.submit()?;

        assert_eq!(ring.completion().len(), 8);
        assert!(ring.completion().is_full());
        assert_eq!(ring.completion().overflow(), 4);
    }

    Ok(())
}
