use std::time::Instant;
use linux_io_uring_sys::__kernel_timespec;
use linux_io_uring::{ opcode, IoUring };


#[test]
fn test_timeout() -> anyhow::Result<()> {
    let mut io_uring = IoUring::new(1)?;

    let ts = __kernel_timespec {
        tv_sec: 1,
        tv_nsec: 0
    };

    let entry = opcode::Timeout::new(&ts);
    unsafe {
        io_uring
            .submission()
            .available()
            .push(entry.build().user_data(0x42))
            .map_err(drop)
            .expect("queue is full");
    }

    let now = Instant::now();
    io_uring.submit_and_wait(1)?;
    let t = now.elapsed();

    assert_eq!(t.as_secs(), 1);

    let entry = io_uring
        .completion()
        .available()
        .next()
        .expect("queue is empty");

    assert_eq!(entry.user_data(), 0x42);

    Ok(())
}
