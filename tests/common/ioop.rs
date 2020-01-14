use std::io;
use io_uring::opcode::{ self, types };
use io_uring::{ squeue, IoUring };


const TEXT: &[u8] = b"hello world!";

pub fn do_write_read(ring: &mut IoUring, target: types::Target) -> anyhow::Result<()> {
    let buf = TEXT.to_vec();
    let mut buf2 = vec![0; TEXT.len()];
    let bufs = [io::IoSlice::new(&buf)];
    let mut bufs2 = [io::IoSliceMut::new(&mut buf2)];

    let write_e = opcode::Writev::new(target, bufs.as_ptr() as *const _, 1);
    let read_e = opcode::Readv::new(target, bufs2.as_mut_ptr() as *mut _, 1);

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
    assert_eq!(cqes[0].result(), 12);
    assert_eq!(cqes[1].result(), 12);

    assert_eq!(buf2, TEXT);

    Ok(())
}
