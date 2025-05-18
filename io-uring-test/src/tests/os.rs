use crate::Test;
use io_uring::{cqueue, opcode, squeue, IoUring};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

pub fn test_waitid<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::WaitId::CODE);
    );

    let mut child = Command::new("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let waitid_e = opcode::WaitId::new(libc::P_PID, child.id(), libc::WEXITED);

    unsafe {
        let mut sq = ring.submission();
        sq.push(&waitid_e.build().user_data(0x110).into())
            .expect("queue is full");
    }

    ring.submit()?;
    thread::sleep(Duration::from_millis(100));
    assert_eq!(ring.completion().len(), 0);

    // close cat
    child.stdin = None;
    child.stdout = None;

    ring.submit_and_wait(1)?;
    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x110);
    assert_eq!(cqes[0].result(), 0);

    Ok(())
}
