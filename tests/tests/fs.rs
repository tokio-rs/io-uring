use std::os::unix::io::AsRawFd;
use io_uring::IoUring;
use io_uring::squeue;
use io_uring::opcode::{ self, types };


pub fn test_file_write_read(ring: &mut IoUring) -> anyhow::Result<()> {
    let fd = tempfile::tempfile()?;
    let fd = types::Fd(fd.as_raw_fd());

    let text = b"The quick brown fox jumps over the lazy dog.";
    let mut output = vec![0; text.len()];

    let write_e = opcode::Write::new(fd, text.as_ptr(), text.len() as _);
    let read_e = opcode::Read::new(fd, output.as_mut_ptr(), output.len() as _);

    unsafe {
        let mut queue = ring.submission().available();
        queue.push(write_e.build().user_data(0x01).flags(squeue::Flags::IO_LINK))
            .ok()
            .expect("queue is full");
        queue.push(read_e.build().user_data(0x02))
            .ok()
            .expect("queue is full");
    }

    ring.submit_and_wait(2)?;

    let cqes = ring
        .completion()
        .available()
        .collect::<Vec<_>>();

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].user_data(), 0x01);
    assert_eq!(cqes[1].user_data(), 0x02);
    assert_eq!(cqes[0].result(), text.len() as i32);
    assert_eq!(cqes[1].result(), text.len() as i32);

    assert_eq!(&output[..cqes[1].result() as usize], text);

    Ok(())
}
