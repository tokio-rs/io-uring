mod common;

use std::os::unix::io::AsRawFd;
use tempfile::tempfile;
use io_uring::opcode::types;
use io_uring::IoUring;
use common::do_write_read;


#[test]
fn test_file() -> anyhow::Result<()> {
    let mut ring = IoUring::new(2)?;

    let fd = tempfile()?;
    let fd = types::Target::Fd(fd.as_raw_fd());

    do_write_read(&mut ring, fd)?;

    Ok(())
}
