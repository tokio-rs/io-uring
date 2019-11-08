use linux_io_uring::{opcode, IoUring};

#[test]
fn test_full() -> anyhow::Result<()> {
    let mut io_uring = IoUring::new(4)?;

    // fill squeue
    unsafe {
        let mut queue = io_uring.submission().available();
        for i in 0..4 {
            let entry = opcode::Nop::new().build().user_data(i);
            queue.push(entry).map_err(drop).expect("queue is full");
        }
        assert!(queue.is_full());
    }
    assert_eq!(io_uring.submit_and_wait(4)?, 4);
    assert!(io_uring.submission().is_empty());

    // fill cqueue
    unsafe {
        let mut queue = io_uring.submission().available();
        for i in 4..8 {
            let entry = opcode::Nop::new().build().user_data(i);
            queue.push(entry).map_err(drop).expect("queue is full");
        }
        assert!(queue.is_full());
    }
    assert_eq!(io_uring.submit_and_wait(8)?, 4);
    assert!(io_uring.submission().is_empty());
    assert!(io_uring.completion().is_full());

    // overflow cqueue
    unsafe {
        let mut queue = io_uring.submission().available();
        for i in 8..12 {
            let entry = opcode::Nop::new().build().user_data(i);
            queue.push(entry).map_err(drop).expect("queue is full");
        }
        assert!(queue.is_full());
    }

    assert_eq!(io_uring.submit_and_wait(4)?, 4);

    assert!(io_uring.submission().is_empty());
    assert!(io_uring.completion().is_full());
    assert_eq!(io_uring.completion().overflow(), 4);

    let mut cqes = io_uring
        .completion()
        .available()
        .map(|entry| entry.user_data())
        .collect::<Vec<_>>();

    cqes.sort();
    assert_eq!(cqes, &[0, 1, 2, 3, 4, 5, 6, 7]);

    Ok(())
}
