use std::io;

use io_uring::cqueue;
use io_uring::opcode;
use io_uring::squeue;
use io_uring::types::{self, AsyncCancelFlags};
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
        .register_sync_cancel(USER_DATA_0, -1, None, AsyncCancelFlags::empty())?;
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
        .register_sync_cancel(USER_DATA_1, -1, None, AsyncCancelFlags::ALL)?;
    ring.submitter().submit_and_wait(2)?;
    let cqe1: cqueue::Entry = ring.completion().next().unwrap().into();
    let cqe2: cqueue::Entry = ring.completion().next().unwrap().into();
    assert_eq!(cqe1.result(), cqe2.result());
    assert_eq!(cqe1.result(), -libc::ECANCELED);

    // Cancel the last two operations by their fd.
    ring.submitter().register_sync_cancel(
        USER_DATA_2,
        0,
        None,
        AsyncCancelFlags::FD | AsyncCancelFlags::ALL,
    )?;
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
        test; // We need at least 5.19 for these tests, UringCmd16 is a proxy for that.
        test.probe.is_supported(opcode::UringCmd16::CODE);
    );
    // Test that we can cancel operations which have not yet been submitted.
    const USER_DATA: u64 = 42u64;

    let mut buf = [0u8; 32];
    let entry = opcode::Read::new(types::Fd(0), buf.as_mut_ptr(), 32).build();
    unsafe { ring.submission().push(&entry.into()).unwrap() };

    // Cancel the operation by user_data, we haven't submitted anything yet.
    ring.submitter()
        .register_sync_cancel(USER_DATA, -1, None, AsyncCancelFlags::empty())?;

    ring.submitter().submit_and_wait(1)?;
    let cqe1: cqueue::Entry = ring.completion().next().unwrap().into();
    assert_eq!(cqe1.result(), -libc::ECANCELED);
    assert_eq!(cqe1.user_data(), USER_DATA);

    Ok(())
}
