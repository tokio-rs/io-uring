use io_uring::{opcode, IoUring, Probe};

pub fn test_nop(ring: &mut IoUring, _probe: &Probe) -> anyhow::Result<()> {
    require!();

    println!("test nop");

    let nop_e = opcode::Nop::new().build().user_data(0x42);

    unsafe {
        let mut queue = ring.submission();
        queue.push(&nop_e).expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes = ring.completion().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x42);
    assert_eq!(cqes[0].result(), 0);

    Ok(())
}

#[cfg(feature = "unstable")]
pub fn test_batch(ring: &mut IoUring, _probe: &Probe) -> anyhow::Result<()> {
    use std::mem::MaybeUninit;

    require!();

    println!("test batch");

    assert!(ring.completion().is_empty());

    unsafe {
        let sqes = vec![opcode::Nop::new().build().user_data(0x09); 5];
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

    let n = ring.completion().fill(&mut cqes);

    assert_eq!(n, 8);

    for entry in cqes.into_iter().take(8) {
        let entry = unsafe { entry.assume_init() };

        assert_eq!(entry.user_data(), 0x09);
    }

    Ok(())
}
