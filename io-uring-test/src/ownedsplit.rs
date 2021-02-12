use io_uring::{opcode, IoUring};
use std::thread;

pub fn main(ring: IoUring) -> anyhow::Result<()> {
    test_nop(ring)?;

    Ok(())
}

fn test_nop(ring: IoUring) -> anyhow::Result<()> {
    println!("test ownedsplit nop");

    let (stu, mut su, mut cu) = ring.owned_split();
    let stu2 = stu.clone();

    let nop_e = opcode::Nop::new().build().user_data(0x42);

    let sj = thread::spawn(move || {
        unsafe {
            su.submission()
                .available()
                .push(nop_e)
                .ok()
                .expect("queue is full");
        }
        stu2.submitter().submit()?;
        Ok(()) as std::io::Result<()>
    });

    stu.submitter().submit_and_wait(1)?;

    let cqes = cu.completion().available().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x42);
    assert_eq!(cqes[0].result(), 0);

    sj.join().unwrap()?;

    Ok(())
}
