use crate::Test;
use io_uring::{cqueue, opcode, squeue, types, IoUring};

pub fn test_nop<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require! {
        test;
    }

    println!("test nop");

    let nop_e = opcode::Nop::new().build().user_data(0x42).into();

    unsafe {
        let mut queue = ring.submission();
        queue.push(&nop_e).expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x42);
    assert_eq!(cqes[0].result(), 0);

    Ok(())
}

pub fn test_batch<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    use std::mem::MaybeUninit;

    require! {
        test;
    }

    println!("test batch");

    assert!(ring.completion().is_empty());

    unsafe {
        let sqes = vec![opcode::Nop::new().build().user_data(0x09).into(); 5];
        let mut sq = ring.submission();

        assert_eq!(sq.capacity(), 8);

        sq.push_multiple(&sqes).unwrap();

        assert_eq!(sq.len(), 5);

        let ret = sq.push_multiple(&sqes);
        assert!(ret.is_err());

        assert_eq!(sq.len(), 5);

        sq.push_multiple(&sqes[..3]).unwrap();
    }

    ring.submit_and_wait(8)?;

    let mut cqes = (0..10).map(|_| MaybeUninit::uninit()).collect::<Vec<_>>();

    let cqes = ring.completion().fill(&mut cqes);

    assert_eq!(cqes.len(), 8);

    for entry in cqes {
        let entry: cqueue::Entry = entry.clone().into();
        assert_eq!(entry.user_data(), 0x09);
    }

    Ok(())
}

pub fn test_queue_split<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require! {
        test;
    }

    println!("test queue_split");

    let (submitter, mut sq, mut cq) = ring.split();

    assert!(sq.is_empty());

    for _ in 0..sq.capacity() {
        unsafe {
            sq.push(&opcode::Nop::new().build().into())
                .expect("queue is full");
        }
    }

    assert!(sq.is_full());

    sq.sync();

    assert_eq!(submitter.submit()?, sq.capacity());
    assert!(sq.is_full());
    sq.sync();
    assert!(sq.is_empty());

    assert!(cq.is_empty());
    cq.sync();
    assert_eq!(cq.len(), sq.capacity());
    assert_eq!(cq.by_ref().count(), sq.capacity());

    cq.sync();

    Ok(())
}

pub fn test_debug_print<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require! {
        test;
    }

    println!("test debug_print");

    let mut sq = ring.submission();
    let num_to_sub = sq.capacity();
    for _ in 0..num_to_sub {
        unsafe {
            sq.push(&opcode::Nop::new().build().user_data(0x42).into())
                .expect("queue is full");
        }
    }
    println!("Full: {:?}", sq);
    drop(sq);

    ring.submit_and_wait(num_to_sub)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), num_to_sub);
    for cqe in cqes {
        assert_eq!(cqe.user_data(), 0x42);
        assert_eq!(cqe.result(), 0);
    }
    println!("Empty: {:?}", ring.submission());

    Ok(())
}

pub fn test_msg_ring_data<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    use std::os::unix::io::AsRawFd;

    require!(
        test;
        test.probe.is_supported(opcode::MsgRingData::CODE);
    );

    println!("test msg_ring_data");

    // Create a new destination ring and send a data message to it through
    // the existing test/source ring. This will generate two completion events,
    // one on each ring.
    let mut dest_ring = IoUring::new(1)?;
    let fd = types::Fd(dest_ring.as_raw_fd());
    let result = 82; // b'R'
    let user_data = 85; // b'U'
    unsafe {
        ring.submission()
            .push(
                &opcode::MsgRingData::new(fd, result, user_data, None)
                    .build()
                    .into(),
            )
            .expect("queue is full");
    }
    ring.submit_and_wait(1)?;

    let source_cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
    assert_eq!(source_cqes.len(), 1);
    assert_eq!(source_cqes[0].user_data(), 0);
    assert_eq!(source_cqes[0].result(), 0);
    assert_eq!(source_cqes[0].flags(), cqueue::CompletionFlags::empty());

    let dest_cqes: Vec<cqueue::Entry> = dest_ring.completion().map(Into::into).collect();
    assert_eq!(dest_cqes.len(), 1);
    assert_eq!(dest_cqes[0].user_data(), user_data);
    assert_eq!(dest_cqes[0].result(), result);
    assert_eq!(dest_cqes[0].flags(), cqueue::CompletionFlags::empty());

    Ok(())
}

pub fn test_msg_ring_send_fd<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    use std::os::unix::io::AsRawFd;

    // This test requires both `IORING_OP_MSG_RING` opcode and `IORING_MSG_SEND_FD` flag.
    // We directly probe for the opcode, but the flag only appeared later
    // (in kernel 6.0) and cannot be probed at runtime.
    // Thus, we additionaly check for the `IORING_OP_SEND_ZC` opcode (>= 6.0)
    // as a proxy instead.
    require!(
        test;
        test.probe.is_supported(opcode::MsgRingData::CODE);
        test.probe.is_supported(opcode::SendZc::CODE);
    );

    println!("test msg_ring_send_fd");

    // Ensure there are no fixed files on the test ring, so that slots
    // allocation will start at 0. Then create a tempfile and register it
    // (at slot 0).
    {
        let _ = ring.submitter().unregister_files();
        let tmp1 = tempfile::tempfile()?;
        ring.submitter()
            .register_files(&[tmp1.as_raw_fd()])
            .unwrap();
    }

    // Create a new destination ring.
    // FD sending only works if the receiving ring already has a slot
    // registered for it. Thus we register some sparse files (slots 0 and 1).
    let mut temp_ring = IoUring::new(8)?;
    temp_ring.submitter().register_files_sparse(2).unwrap();

    // Send the fixed file from the source ring (on slot 0)
    // to the temp ring (to slot 1).
    // This will generate two completion events, one on each ring.
    unsafe {
        let fd = types::Fd(temp_ring.as_raw_fd());
        let dest_slot = types::DestinationSlot::try_from_slot_target(1).unwrap();
        ring.submission()
            .push(
                &opcode::MsgRingSendFd::new(fd, types::Fixed(0), dest_slot, 22)
                    .build()
                    .into(),
            )
            .expect("queue is full");
    }
    ring.submit_and_wait(1)?;
    {
        let source_cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
        assert_eq!(source_cqes.len(), 1);
        assert_eq!(source_cqes[0].user_data(), 0);
        assert_eq!(source_cqes[0].result(), 0);
        assert_eq!(source_cqes[0].flags(), cqueue::CompletionFlags::empty());

        let dest_cqes: Vec<cqueue::Entry> = temp_ring.completion().map(Into::into).collect();
        assert_eq!(dest_cqes.len(), 1);
        assert_eq!(dest_cqes[0].user_data(), 22);
        assert_eq!(dest_cqes[0].result(), 0);
        assert_eq!(dest_cqes[0].flags(), cqueue::CompletionFlags::empty());
    }

    // Unregister the fixed files from the source ring, then reserve some empty slots
    // (0 through 3).
    ring.submitter().unregister_files().unwrap();
    ring.submitter().register_files_sparse(4).unwrap();

    // Send back the fixed file from the temp ring (on slot 1)
    // to the temp ring (to slot 2).
    // This will again generate two completion events, one on each ring.
    unsafe {
        let fd = types::Fd(ring.as_raw_fd());
        let dest_slot = types::DestinationSlot::try_from_slot_target(2).unwrap();
        temp_ring
            .submission()
            .push(&opcode::MsgRingSendFd::new(fd, types::Fixed(1), dest_slot, 44).build())
            .expect("queue is full");
    }
    temp_ring.submit_and_wait(1)?;
    {
        let source_cqes: Vec<cqueue::Entry> = temp_ring.completion().map(Into::into).collect();
        assert_eq!(source_cqes.len(), 1);
        assert_eq!(source_cqes[0].user_data(), 0);
        assert_eq!(source_cqes[0].result(), 0);
        assert_eq!(source_cqes[0].flags(), cqueue::CompletionFlags::empty());

        let dest_cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
        assert_eq!(dest_cqes.len(), 1);
        assert_eq!(dest_cqes[0].user_data(), 44);
        assert_eq!(dest_cqes[0].result(), 0);
        assert_eq!(dest_cqes[0].flags(), cqueue::CompletionFlags::empty());
    }

    // Unregister the fixed files from both rings, then repeat again to
    // ensure that all previous files have been unregistered (on empty rings
    // the operation will fail with `-ENXIO`).
    ring.submitter().unregister_files().unwrap();
    temp_ring.submitter().unregister_files().unwrap();
    ring.submitter().unregister_files().unwrap_err();
    temp_ring.submitter().unregister_files().unwrap_err();

    Ok(())
}
