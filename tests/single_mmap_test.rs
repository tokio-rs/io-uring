use linux_io_uring::{ opcode, squeue, Builder, FeatureFlag };


#[test]
fn test_single_mmap_nop() -> anyhow::Result<()> {
    let mut io_uring = Builder::new(2)
        .feature_flags(FeatureFlag::SINGLE_MMAP)
        .build()?;

    unsafe {
        io_uring
            .submission()
            .available()
            .push(opcode::Nop::new().build().user_data(0x42))
            .map_err(drop)
            .expect("queue is full");

        io_uring
            .submission()
            .available()
            .push(opcode::Nop::new().build().user_data(0x43).flags(squeue::Flag::IO_DRAIN))
            .map_err(drop)
            .expect("queue is full");
    }

    io_uring.submit_and_wait(2)?;

    let cqes = io_uring
        .completion()
        .available()
        .map(|entry| entry.user_data())
        .collect::<Vec<_>>();

    assert_eq!(cqes, [0x42, 0x43]);

    Ok(())
}
