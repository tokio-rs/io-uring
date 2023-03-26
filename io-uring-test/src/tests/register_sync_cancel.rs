use std::io;

use io_uring::cqueue;
use io_uring::opcode;
use io_uring::squeue;
use io_uring::types;
use io_uring::types::MatchOn;
use io_uring::IoUring;

use crate::Test;

pub fn test_register_sync_cancel<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> io::Result<()> {
    require!(
        test; // We need at least 5.19 for these tests, UringCmd16 is a proxy for that.
        test.probe.is_supported(opcode::UringCmd16::CODE);
    );
    // Single op, canceled by the user_data
    const USER_DATA_0: u64 = 42u64;
    // 2 operations, canceled by the user_data
    const USER_DATA_1: u64 = 43u64;
    // 2 operations, canceled by the fd.
    const USER_DATA_2: u64 = 44u64;

    // Start reads for user_data_0
    let mut buf = [0u8; 32];
    for i in 0..5 {
        let mut entry = opcode::Read::new(types::Fd(0), buf.as_mut_ptr(), 32).build();

        if i == 0 {
            entry = entry.user_data(USER_DATA_0);
        } else if (1..3).contains(&i) {
            entry = entry.user_data(USER_DATA_1);
        } else if (3..5).contains(&i) {
            entry = entry.user_data(USER_DATA_2);
        }
        unsafe { ring.submission().push(&entry.into()).unwrap() };
    }
    // Submit all 5 operations.
    assert_eq!(5, ring.submit()?);

    // Cancel the first operation by user_data
    ring.submitter()
        .register_sync_cancel(MatchOn::user_data(USER_DATA_0), None)?;
    ring.submitter().submit_and_wait(1)?;
    let cqe: cqueue::Entry = ring.completion().next().unwrap().into();
    assert_eq!(
        cqe.result(),
        -libc::ECANCELED,
        "operation should have been canceled"
    );
    assert_eq!(cqe.user_data(), USER_DATA_0);

    // Cancel the second two operations by their user data flags.
    ring.submitter()
        .register_sync_cancel(MatchOn::user_data(USER_DATA_0).match_any(), None)?;
    ring.submitter().submit_and_wait(2)?;
    let cqe1: cqueue::Entry = ring.completion().next().unwrap().into();
    let cqe2: cqueue::Entry = ring.completion().next().unwrap().into();
    assert_eq!(cqe1.result(), cqe2.result());
    assert_eq!(cqe1.result(), -libc::ECANCELED);

    // Cancel the last two operations by their fd.
    ring.submitter()
        .register_sync_cancel(MatchOn::fd(types::Fd(0)).match_any(), None)?;
    ring.submitter().submit_and_wait(2)?;
    let cqe1: cqueue::Entry = ring.completion().next().unwrap().into();
    let cqe2: cqueue::Entry = ring.completion().next().unwrap().into();
    assert_eq!(cqe1.result(), cqe2.result());
    assert_eq!(cqe1.result(), -libc::ECANCELED);

    Ok(())
}

pub fn test_register_sync_cancel_unsubmitted<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> io::Result<()> {
    require!(
        test; // We need at least 6.0 to use `IORING_REGISTER_SYNC_CANCEL`. opcode::SendZc is a proxy for that requirement.
        test.probe.is_supported(opcode::SendZc::CODE);
    );
    // Test that we can cancel operations which have not yet been submitted.
    const USER_DATA: u64 = 42u64;

    let mut buf = [0u8; 32];
    let entry = opcode::Read::new(types::Fd(0), buf.as_mut_ptr(), 32)
        .build()
        .user_data(USER_DATA);
    unsafe { ring.submission().push(&entry.into()).unwrap() };

    // Cancel the operation by user_data, we haven't submitted anything yet.
    let result = ring
        .submitter()
        .register_sync_cancel(MatchOn::user_data(USER_DATA), None);
    assert!(
        matches!(result.err().unwrap().kind(), io::ErrorKind::NotFound),
        "the operation should not complete because the entry has not been submitted"
    );

    // Submit the operation, and retry the cancel operation. It should succeed.
    assert_eq!(1, ring.submitter().submit()?);
    ring.submitter()
        .register_sync_cancel(MatchOn::user_data(USER_DATA), None)?;

    ring.submitter().submit_and_wait(1)?;
    let cqe1: cqueue::Entry = ring.completion().next().unwrap().into();
    assert_eq!(cqe1.result(), -libc::ECANCELED);
    assert_eq!(cqe1.user_data(), USER_DATA);

    Ok(())
}
