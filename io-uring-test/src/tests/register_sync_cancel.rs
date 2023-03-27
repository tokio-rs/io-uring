use std::io;
use std::mem;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::fd::OwnedFd;

use io_uring::cqueue;
use io_uring::opcode;
use io_uring::squeue;
use io_uring::types;
use io_uring::types::CancelBuilder;
use io_uring::IoUring;

use crate::Test;

pub fn test_register_sync_cancel<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> io::Result<()> {
    require!(
        test; // We need at least 6.0 to use `IORING_REGISTER_SYNC_CANCEL`. opcode::SendZc is a proxy for that requirement.
        test.probe.is_supported(opcode::SendZc::CODE);
    );
    let fd_1 = get_eventfd();
    let fd_2 = get_eventfd();
    let fixed_fd_3 = get_eventfd();

    let mut registered_files = [0; 1];
    registered_files[0] = fixed_fd_3.as_raw_fd();
    ring.submitter().register_files(&registered_files)?;

    // Single op, canceled by the user_data. Reads from fd_1
    const USER_DATA_0: u64 = 42u64;
    // 2 operations, canceled by the user_data. Reads from fd_1
    const USER_DATA_1: u64 = 43u64;
    // 2 operations, canceled by the fd. Reads from fd_1
    const USER_DATA_2: u64 = 44u64;
    // 3 operations, reading from fixed_fd_3.
    const USER_DATA_3: u64 = 45u64;
    // 3 operations, canceled by ANY. Reads from fd_2.
    const USER_DATA_4: u64 = 46u64;

    let mut buf = [0u8; 32];
    let mut nr_submitted = 0;
    for i in 0..11 {
        let entry;
        if i == 0 {
            entry = opcode::Read::new(types::Fd(fd_1.as_raw_fd()), buf.as_mut_ptr(), 32)
                .build()
                .user_data(USER_DATA_0);
        } else if (1..3).contains(&i) {
            entry = opcode::Read::new(types::Fd(fd_1.as_raw_fd()), buf.as_mut_ptr(), 32)
                .build()
                .user_data(USER_DATA_1);
        } else if (3..5).contains(&i) {
            entry = opcode::Read::new(types::Fd(fd_1.as_raw_fd()), buf.as_mut_ptr(), 32)
                .build()
                .user_data(USER_DATA_2);
        } else if (5..8).contains(&i) {
            entry = opcode::Read::new(types::Fixed(0), buf.as_mut_ptr(), 32)
                .build()
                .user_data(USER_DATA_3);
        } else {
            entry = opcode::Read::new(types::Fd(fd_2.as_raw_fd()), buf.as_mut_ptr(), 32)
                .build()
                .user_data(USER_DATA_4);
        }
        unsafe { ring.submission().push(&entry.into()).unwrap() };
        nr_submitted += ring.submit()?;
    }
    // 11 operations should have been submitted.
    assert_eq!(11, nr_submitted);

    // Cancel the first operation by user_data.
    ring.submitter()
        .register_sync_cancel(None, CancelBuilder::new().user_data(USER_DATA_0))?;
    let completions = wait_get_completions(ring, 1).unwrap();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].user_data(), USER_DATA_0);
    assert_eq!(completions[0].result(), -libc::ECANCELED);

    // Cancel the second and third operation by user_data.
    ring.submitter()
        .register_sync_cancel(None, CancelBuilder::new().user_data(USER_DATA_1).all())?;
    let completions = wait_get_completions(ring, 2).unwrap();
    assert_eq!(completions.len(), 2);
    for completion in completions {
        assert_eq!(completion.user_data(), USER_DATA_1);
        assert_eq!(completion.result(), -libc::ECANCELED);
    }

    // Cancel the fourth and fifth operation by fd.
    ring.submitter().register_sync_cancel(
        None,
        CancelBuilder::new().fd(types::Fd(fd_1.as_raw_fd())).all(),
    )?;
    let completions = wait_get_completions(ring, 2).unwrap();
    assert_eq!(completions.len(), 2);
    for completion in completions {
        assert_eq!(completion.user_data(), USER_DATA_2);
        assert_eq!(completion.result(), -libc::ECANCELED);
    }

    // Cancel one of the fixed_fd operations by the fixed_fd.
    ring.submitter()
        .register_sync_cancel(None, CancelBuilder::new().fd(types::Fixed(0)))?;
    let completions = wait_get_completions(ring, 1).unwrap();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].user_data(), USER_DATA_3);
    assert_eq!(completions[0].result(), -libc::ECANCELED);

    // Cancel the two remaining fixed_fd operations by the fixed_fd.
    ring.submitter()
        .register_sync_cancel(None, CancelBuilder::new().fd(types::Fixed(0)).all())?;
    let completions = wait_get_completions(ring, 2).unwrap();
    for completion in completions {
        assert_eq!(completion.user_data(), USER_DATA_3);
        assert_eq!(completion.result(), -libc::ECANCELED);
    }

    // Cancel all of the remaining requests, should be 3 outstanding.
    ring.submitter()
        .register_sync_cancel(None, CancelBuilder::new())?;
    let completions = wait_get_completions(ring, 1).unwrap();
    assert_eq!(completions.len(), 3);
    for completion in completions {
        assert_eq!(completion.user_data(), USER_DATA_4);
        assert_eq!(completion.result(), -libc::ECANCELED);
    }

    ring.submitter().unregister_files()?;
    Ok(())
}

pub fn test_register_sync_cancel_any<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> io::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::SendZc::CODE);
    );

    // Test that CancelBuilder::new().all() cancels all requests in the ring.
    let fd_1 = get_eventfd();
    const START_USER_DATA: u64 = 47u64;
    let mut buf = [0u8; 32];

    for i in 0..3 {
        let entry = opcode::Read::new(types::Fd(fd_1.as_raw_fd()), buf.as_mut_ptr(), 32)
            .build()
            .user_data(START_USER_DATA + i);
        unsafe { ring.submission().push(&entry.into()).unwrap() };
    }
    // Submit all 3 operations.
    assert_eq!(3, ring.submit()?);

    // Cancel all of the requests by supplying CancelBuilder::new().all(). This should result in
    // flags IORING_ASYNC_CANCEL_ALL | IORING_ASYNC_CANCEL_ANY, which match all in-flight requsts.
    //
    // Note: IORING_ASYNC_CANCEL_ALL makes no difference here, IORING_ASYNC_CANCEL_ANY will behave
    //       the same without it. This test just verifies that the builder works as expected.
    ring.submitter()
        .register_sync_cancel(None, CancelBuilder::new().all())?;

    let completions = wait_get_completions(ring, 5).unwrap();
    assert_eq!(completions.len(), 3);
    for completion in completions.iter() {
        assert_eq!(completion.result(), -libc::ECANCELED);
    }
    let mut user_data_entries = completions
        .into_iter()
        .map(|c| c.user_data())
        .collect::<Vec<u64>>();
    user_data_entries.sort();
    assert_eq!(
        user_data_entries,
        vec![START_USER_DATA, START_USER_DATA + 1, START_USER_DATA + 2,]
    );

    Ok(())
}

pub fn test_register_sync_cancel_unsubmitted<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> io::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::SendZc::CODE);
    );

    let fd_1 = get_eventfd();

    // Try to cancel an operation which has not yet been submitted, but has been pushed to the SQ.
    // This should result in an error from register_sync_cancel and the operation should not be cancelled.
    const USER_DATA: u64 = 47u64;

    let mut buf = [0u8; 32];
    let entry = opcode::Read::new(types::Fd(fd_1.as_raw_fd()), buf.as_mut_ptr(), 32)
        .build()
        .user_data(USER_DATA);
    unsafe { ring.submission().push(&entry.into()).unwrap() };

    // Cancel the operation by user_data, we haven't submitted anything yet. We should get an error.
    let result = ring
        .submitter()
        .register_sync_cancel(None, CancelBuilder::new().user_data(USER_DATA));
    assert!(
        matches!(result.err().unwrap().kind(), io::ErrorKind::NotFound),
        "the operation should not complete because the entry has not been submitted"
    );

    // Submit the operation, and retry the cancel operation. It should succeed.
    assert_eq!(1, ring.submitter().submit()?);
    ring.submitter()
        .register_sync_cancel(None, CancelBuilder::new().user_data(USER_DATA))?;

    let completions = wait_get_completions(ring, 1)?;
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].user_data(), USER_DATA);
    assert_eq!(completions[0].result(), -libc::ECANCELED);

    Ok(())
}

/// Blocks for a short amount of time, waiting for completions to arrive.
///
/// Returns all completions that have arrived.
fn wait_get_completions<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    want: usize,
) -> io::Result<Vec<cqueue::Entry>> {
    let ts = types::Timespec::new().nsec(1_000_000).sec(0);
    let args = types::SubmitArgs::new().timespec(&ts);

    ring.submitter().submit_with_args(want, &args)?;
    Ok(ring
        .completion()
        .map(Into::into)
        .collect::<Vec<cqueue::Entry>>())
}

/// Setup an eventfd which we can use for a blocking read.
fn get_eventfd() -> OwnedFd {
    let fd = unsafe { libc::eventfd(0, 0) };
    assert!(fd >= 0);
    unsafe { OwnedFd::from_raw_fd(fd) }
}
