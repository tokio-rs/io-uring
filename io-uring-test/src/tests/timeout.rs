use io_uring::{opcode, types, IoUring};
use std::time::Instant;

pub fn test_timeout(ring: &mut IoUring) -> anyhow::Result<()> {
    println!("test timeout");

    // add timeout

    let ts = types::Timespec::new().sec(1);
    let timeout_e = opcode::Timeout::new(&ts);

    unsafe {
        let mut queue = ring.submission().available();
        queue
            .push(timeout_e.build().user_data(0x09))
            .ok()
            .expect("queue is full");
    }

    let start = Instant::now();
    ring.submit_and_wait(1)?;

    assert_eq!(start.elapsed().as_secs(), 1);

    let cqes = ring.completion().available().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x09);
    assert_eq!(cqes[0].result(), -libc::ETIME);

    // add timeout but no

    let ts = types::Timespec::new().sec(1);
    let timeout_e = opcode::Timeout::new(&ts);
    let nop_e = opcode::Nop::new();

    unsafe {
        let mut queue = ring.submission().available();
        queue
            .push(timeout_e.build().user_data(0x0a))
            .ok()
            .expect("queue is full");
        queue
            .push(nop_e.build().user_data(0x0b))
            .ok()
            .expect("queue is full");
    }

    // nop

    let start = Instant::now();
    ring.submit_and_wait(1)?;

    assert_eq!(start.elapsed().as_secs(), 0);

    let cqes = ring.completion().available().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x0b);
    assert_eq!(cqes[0].result(), 0);

    // timeout

    ring.submit_and_wait(1)?;

    assert_eq!(start.elapsed().as_secs(), 1);

    let cqes = ring.completion().available().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x0a);
    assert_eq!(cqes[0].result(), -libc::ETIME);

    Ok(())
}

pub fn test_timeout_count(ring: &mut IoUring) -> anyhow::Result<()> {
    println!("test timeout_count");

    let ts = types::Timespec::new().sec(1);
    let timeout_e = opcode::Timeout::new(&ts).count(1);
    let nop_e = opcode::Nop::new();

    unsafe {
        let mut queue = ring.submission().available();
        queue
            .push(timeout_e.build().user_data(0x0c))
            .ok()
            .expect("queue is full");
        queue
            .push(nop_e.build().user_data(0x0d))
            .ok()
            .expect("queue is full");
    }

    let start = Instant::now();
    ring.submit_and_wait(2)?;

    assert_eq!(start.elapsed().as_secs(), 0);

    let mut cqes = ring.completion().available().collect::<Vec<_>>();
    cqes.sort_by_key(|cqe| cqe.user_data());

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].user_data(), 0x0c);
    assert_eq!(cqes[1].user_data(), 0x0d);
    assert_eq!(cqes[0].result(), 0);
    assert_eq!(cqes[1].result(), 0);

    Ok(())
}

pub fn test_timeout_remove(ring: &mut IoUring) -> anyhow::Result<()> {
    println!("test timeout_remove");

    // add timeout

    let ts = types::Timespec::new().sec(1);
    let timeout_e = opcode::Timeout::new(&ts);

    unsafe {
        let mut queue = ring.submission().available();
        queue
            .push(timeout_e.build().user_data(0x10))
            .ok()
            .expect("queue is full");
    }

    ring.submit()?;

    // remove timeout

    let timeout_e = opcode::TimeoutRemove::new(0x10);

    unsafe {
        let mut queue = ring.submission().available();
        queue
            .push(timeout_e.build().user_data(0x11))
            .ok()
            .expect("queue is full");
    }

    let start = Instant::now();
    ring.submit_and_wait(2)?;

    assert_eq!(start.elapsed().as_secs(), 0);

    let mut cqes = ring.completion().available().collect::<Vec<_>>();
    cqes.sort_by_key(|cqe| cqe.user_data());

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].user_data(), 0x10);
    assert_eq!(cqes[1].user_data(), 0x11);
    assert_eq!(cqes[0].result(), -libc::ECANCELED);
    assert_eq!(cqes[1].result(), 0);

    Ok(())
}

pub fn test_timeout_cancel(ring: &mut IoUring) -> anyhow::Result<()> {
    println!("test timeout_cancel");

    // add timeout

    let ts = types::Timespec::new().sec(1);
    let timeout_e = opcode::Timeout::new(&ts);

    unsafe {
        let mut queue = ring.submission().available();
        queue
            .push(timeout_e.build().user_data(0x10))
            .ok()
            .expect("queue is full");
    }

    ring.submit()?;

    // remove timeout

    let timeout_e = opcode::AsyncCancel::new(0x10);

    unsafe {
        let mut queue = ring.submission().available();
        queue
            .push(timeout_e.build().user_data(0x11))
            .ok()
            .expect("queue is full");
    }

    let start = Instant::now();
    ring.submit_and_wait(2)?;

    assert_eq!(start.elapsed().as_secs(), 0);

    let mut cqes = ring.completion().available().collect::<Vec<_>>();
    cqes.sort_by_key(|cqe| cqe.user_data());

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].user_data(), 0x10);
    assert_eq!(cqes[1].user_data(), 0x11);
    assert_eq!(cqes[0].result(), -libc::ECANCELED);
    assert_eq!(cqes[1].result(), 0);

    Ok(())
}
