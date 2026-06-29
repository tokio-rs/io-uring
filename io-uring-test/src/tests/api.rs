use crate::Test;
use io_uring::IoUring;
use io_uring::{cqueue, squeue};

pub fn test_sendness<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    _test: &Test,
) -> anyhow::Result<()> {
    fn assert_send<T: Send>(t: T) -> T {
        t
    }

    let ring = assert_send(ring);

    let (submitter, sq, cq) = ring.split();
    assert_send(submitter);
    assert_send(sq);
    assert_send(cq);
    Ok(())
}
