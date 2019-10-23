use std::thread;
use std::sync::Arc;
use std::io::{ self, Read };
use std::os::unix::io::AsRawFd;
use tempfile::tempfile;
use linux_io_uring::{ opcode, IoUring };


#[test]
fn test_fs() -> io::Result<()> {
    const TEXT: &[u8] = b"hello world!";

    let io_uring = Arc::new(IoUring::new(2)?.concurrent());
    let io_uring2 = io_uring.clone();
    let io_uring3 = io_uring.clone();

    let mut fd = tempfile()?;
    let raw_fd = fd.as_raw_fd();

    let j = thread::spawn(move || {
        // writev submit
        let entry = opcode::Writev::new(
            opcode::Target::Fd(raw_fd),
            [io::IoSlice::new(TEXT)].as_ptr() as *const _,
            1
        );

        unsafe {
            io_uring2
                .submission()
                .push(entry.build().user_data(0x42))
                .map_err(drop)
                .expect("queue is full");
        }

        io_uring2.submit().unwrap();
    });

    let j2 = thread::spawn(move || {
        // writev submit
        let entry = opcode::Writev::new(
            opcode::Target::Fd(raw_fd),
            [io::IoSlice::new(TEXT)].as_ptr() as *const _,
            1
        );
        let entry = entry.offset(TEXT.len() as i64);

        unsafe {
            io_uring3
                .submission()
                .push(entry.build().user_data(0x43))
                .map_err(drop)
                .expect("queue is full");
        }

        io_uring3.submit().unwrap();
    });

    io_uring.submit_and_wait(2)?;

    // writev complete
    let entry = io_uring
        .completion()
        .pop()
        .expect("queue is empty");
    let data1 = entry.user_data();

    // writev complete
    let entry = io_uring
        .completion()
        .pop()
        .expect("queue is empty");
    let data2 = entry.user_data();

    assert!(data1 == 0x42 || data1 == 0x43);
    assert!(data2 == 0x42 || data2 == 0x43);
    assert_ne!(data1, data2);

    let mut buf = Vec::new();
    fd.read_to_end(&mut buf)?;
    assert_eq!(&buf[..TEXT.len()], TEXT);
    assert_eq!(&buf[TEXT.len()..], TEXT);

    j.join().unwrap();
    j2.join().unwrap();

    Ok(())
}

#[test]
fn push_then_conc() -> io::Result<()> {
    let mut io_uring = IoUring::new(2)?;

    unsafe {
        io_uring
            .submission()
            .available()
            .push(opcode::Nop::new().build())
            .map_err(drop)
            .expect("queue is full");
    }

    assert_eq!(io_uring.submission().len(), 1);


    let io_uring = io_uring.concurrent();

    assert_eq!(io_uring.submission().len(), 1);

    unsafe {
        io_uring
            .submission()
            .push(opcode::Nop::new().build())
            .map_err(drop)
            .expect("queue is full");
    }

    assert_eq!(io_uring.submission().len(), 2);

    Ok(())
}
