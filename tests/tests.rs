#[path = "tests/fs.rs"]
mod fs;

use io_uring::{ opcode, IoUring, Probe };


#[test]
fn test_all() -> anyhow::Result<()> {
    let mut ring = IoUring::new(8)?;
    let mut probe = Probe::new();

    if ring.submitter().register_probe(&mut probe).is_err() {
        panic!("No probe supported");
    }

    if probe.is_supported(opcode::Write::CODE) && probe.is_supported(opcode::Read::CODE) {
        fs::test_file_write_read(&mut ring)?;
    }

    Ok(())
}
