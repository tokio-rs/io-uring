use io_uring::opcode;
use io_uring::IoUring;

pub fn test_nop(ring: &mut IoUring) -> anyhow::Result<()> {
    println!("test_nop");

    let nop_e = opcode::Nop::new().build().user_data(0x42);

    unsafe {
        let mut queue = ring.submission().available();
        queue.push(nop_e).ok().expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes = ring.completion().available().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x42);
    assert_eq!(cqes[0].result(), 0);

    Ok(())
}
