use crate::Test;
use io_uring::{opcode, types, IoUring};
use std::time::Instant;

pub fn test_timeout(ring: &mut IoUring, test: &Test) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Timeout::CODE);
    );

    println!("test timeout");

    // add timeout

    let ts = types::Timespec::new().sec(1);
    let timeout_e = opcode::Timeout::new(&ts);

    unsafe {
        let mut queue = ring.submission();
        queue
            .push(&timeout_e.build().user_data(0x09))
            .expect("queue is full");
    }

    let start = Instant::now();
    ring.submit_and_wait(1)?;

    assert_eq!(start.elapsed().as_secs(), 1);

    let cqes = ring.completion().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x09);
    assert_eq!(cqes[0].result(), -libc::ETIME);

    // add timeout but no

    let ts = types::Timespec::new().sec(1);
    let timeout_e = opcode::Timeout::new(&ts);
    let nop_e = opcode::Nop::new();

    unsafe {
        let mut queue = ring.submission();
        queue
            .push(&timeout_e.build().user_data(0x0a))
            .expect("queue is full");
        queue
            .push(&nop_e.build().user_data(0x0b))
            .expect("queue is full");
    }

    // nop

    let start = Instant::now();
    ring.submit_and_wait(1)?;

    assert_eq!(start.elapsed().as_secs(), 0);

    let cqes = ring.completion().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x0b);
    assert_eq!(cqes[0].result(), 0);

    // timeout

    ring.submit_and_wait(1)?;

    assert_eq!(start.elapsed().as_secs(), 1);

    let cqes = ring.completion().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x0a);
    assert_eq!(cqes[0].result(), -libc::ETIME);

    Ok(())
}

pub fn test_timeout_count(ring: &mut IoUring, test: &Test) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Timeout::CODE);
    );

    println!("test timeout_count");

    let ts = types::Timespec::new().sec(1);
    let timeout_e = opcode::Timeout::new(&ts).count(1);
    let nop_e = opcode::Nop::new();

    unsafe {
        let mut queue = ring.submission();
        queue
            .push(&timeout_e.build().user_data(0x0c))
            .expect("queue is full");
        queue
            .push(&nop_e.build().user_data(0x0d))
            .expect("queue is full");
    }

    let start = Instant::now();
    ring.submit_and_wait(2)?;

    assert_eq!(start.elapsed().as_secs(), 0);

    let mut cqes = ring.completion().collect::<Vec<_>>();
    cqes.sort_by_key(|cqe| cqe.user_data());

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].user_data(), 0x0c);
    assert_eq!(cqes[1].user_data(), 0x0d);
    assert_eq!(cqes[0].result(), 0);
    assert_eq!(cqes[1].result(), 0);

    Ok(())
}

pub fn test_timeout_remove(ring: &mut IoUring, test: &Test) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Timeout::CODE);
        test.probe.is_supported(opcode::TimeoutRemove::CODE);
    );

    println!("test timeout_remove");

    // add timeout

    let ts = types::Timespec::new().sec(1);
    let timeout_e = opcode::Timeout::new(&ts);

    unsafe {
        let mut queue = ring.submission();
        queue
            .push(&timeout_e.build().user_data(0x10))
            .expect("queue is full");
    }

    ring.submit()?;

    // remove timeout

    let timeout_e = opcode::TimeoutRemove::new(0x10);

    unsafe {
        let mut queue = ring.submission();
        queue
            .push(&timeout_e.build().user_data(0x11))
            .expect("queue is full");
    }

    let start = Instant::now();
    ring.submit_and_wait(2)?;

    assert_eq!(start.elapsed().as_secs(), 0);

    let mut cqes = ring.completion().collect::<Vec<_>>();
    cqes.sort_by_key(|cqe| cqe.user_data());

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].user_data(), 0x10);
    assert_eq!(cqes[1].user_data(), 0x11);
    assert_eq!(cqes[0].result(), -libc::ECANCELED);
    assert_eq!(cqes[1].result(), 0);

    Ok(())
}

pub fn test_timeout_cancel(ring: &mut IoUring, test: &Test) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Timeout::CODE);
        test.probe.is_supported(opcode::AsyncCancel::CODE);
    );

    println!("test timeout_cancel");

    // add timeout

    let ts = types::Timespec::new().sec(1);
    let timeout_e = opcode::Timeout::new(&ts);

    unsafe {
        let mut queue = ring.submission();
        queue
            .push(&timeout_e.build().user_data(0x10))
            .expect("queue is full");
    }

    ring.submit()?;

    // remove timeout

    let timeout_e = opcode::AsyncCancel::new(0x10);

    unsafe {
        let mut queue = ring.submission();
        queue
            .push(&timeout_e.build().user_data(0x11))
            .expect("queue is full");
    }

    let start = Instant::now();
    ring.submit_and_wait(2)?;

    assert_eq!(start.elapsed().as_secs(), 0);

    let mut cqes = ring.completion().collect::<Vec<_>>();
    cqes.sort_by_key(|cqe| cqe.user_data());

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].user_data(), 0x10);
    assert_eq!(cqes[1].user_data(), 0x11);
    assert_eq!(cqes[0].result(), -libc::ECANCELED);
    assert_eq!(cqes[1].result(), 0);

    Ok(())
}

pub fn test_timeout_abs(ring: &mut IoUring, test: &Test) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Timeout::CODE);
    );

    println!("test timeout_abs");

    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };

    let ret = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) };

    assert_eq!(ret, 0);

    let ts = types::Timespec::new()
        .sec(now.tv_sec as u64 + 2)
        .nsec(now.tv_nsec as u32);

    let timeout_e = opcode::Timeout::new(&ts).flags(types::TimeoutFlags::ABS);

    unsafe {
        let mut queue = ring.submission();
        queue
            .push(&timeout_e.build().user_data(0x19))
            .expect("queue is full");
    }

    let start = Instant::now();
    ring.submit_and_wait(1)?;

    assert!(start.elapsed().as_secs() >= 1);

    let cqes = ring.completion().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x19);
    assert_eq!(cqes[0].result(), -libc::ETIME);

    Ok(())
}

#[cfg(feature = "unstable")]
pub fn test_timeout_submit_args(ring: &mut IoUring, test: &Test) -> anyhow::Result<()> {
    require! {
        test;
        ring.params().is_feature_ext_arg();
    };

    println!("test timeout_submit_args");

    let ts = types::Timespec::new().sec(1);
    let args = types::SubmitArgs::new().timespec(&ts);

    // timeout

    let start = Instant::now();
    match ring.submitter().submit_with_args(1, &args) {
        Ok(_) => panic!(),
        Err(ref err) if err.raw_os_error() == Some(libc::ETIME) => (),
        Err(err) => return Err(err.into()),
    }
    assert_eq!(start.elapsed().as_secs(), 1);

    let cqes = ring.completion().collect::<Vec<_>>();

    assert!(cqes.is_empty());

    // no timeout

    let nop_e = opcode::Nop::new();

    unsafe {
        ring.submission()
            .push(&nop_e.build().user_data(0x1c))
            .expect("queue is full");
    }

    let start = Instant::now();
    ring.submitter().submit_with_args(1, &args)?;
    assert_eq!(start.elapsed().as_secs(), 0);

    let cqes = ring.completion().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x1c);
    assert_eq!(cqes[0].result(), 0);

    Ok(())
}
