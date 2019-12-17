use linux_io_uring::{ opcode, IoUring };


#[test]
fn test_full() -> anyhow::Result<()> {
    let mut io_uring = IoUring::new(4)?;

    // squeue full
    {
        let mut queue = io_uring.submission().available();

        assert!(queue.is_empty());

        for _ in 0..queue.capacity() {
            unsafe {
                queue.push(opcode::Nop::new().build())
                    .map_err(drop)
                    .expect("queue is full");
            }
        }

        assert!(queue.is_full());
    }

    assert!(io_uring.submission().is_full());
    io_uring.submit()?;


    #[cfg(feature = "concurrent")] {
        // concurrent squeue full
        let io_uring = io_uring.concurrent();
        let queue = io_uring.submission();

        assert!(queue.is_empty());

        for _ in 0..queue.capacity() {
            unsafe {
                queue.push(opcode::Nop::new().build())
                    .map_err(drop)
                    .expect("queue is full");
            }
        }

        assert!(queue.is_full());
    }

    Ok(())
}
