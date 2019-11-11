use std::thread;
use std::os::unix::io::AsRawFd;
use std::io::{ self, Read, Write };
use std::net::{ SocketAddr, TcpListener, TcpStream };
use lazy_static::lazy_static;
use linux_io_uring::{ opcode, squeue, IoUring };


const TEXT: &[u8] = b"hello world!";

lazy_static!{
    static ref TEST_SERVER: SocketAddr = {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();

        thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut buf = [0; 12];
                assert_eq!(TEXT.len(), buf.len());
                stream.read_exact(&mut buf).unwrap();
                stream.write_all(&buf).unwrap();
            }
        });

        addr
    };
}

pub fn echo_server() -> &'static SocketAddr {
    &*TEST_SERVER
}


#[test]
fn test_tcp_write_and_read() -> anyhow::Result<()> {
    let addr = echo_server();
    let mut io_uring = IoUring::new(4)?;

    let stream = TcpStream::connect(addr)?;
    let fd = stream.as_raw_fd();

    let mut buf = vec![0; TEXT.len()];

    let write_entry =
        opcode::Writev::new(
            opcode::Target::Fd(fd),
            [io::IoSlice::new(TEXT)].as_ptr() as *const _,
            1
        )
        .build();
    let read_entry =
        opcode::Readv::new(
            opcode::Target::Fd(fd),
            [io::IoSliceMut::new(&mut buf)].as_mut_ptr() as *mut _,
            1
        )
        .build()
        .flags(squeue::Flags::IO_LINK);

    unsafe {
        let mut queue = io_uring.submission().available();
        queue.push(write_entry.user_data(0x01))
            .map_err(drop)
            .expect("queue is full");
        queue.push(read_entry.user_data(0x02))
            .map_err(drop)
            .expect("queue is full");
    }

    io_uring.submit_and_wait(2)?;

    let cqes = io_uring
        .completion()
        .available()
        .collect::<Vec<_>>();

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].user_data(), 0x01);
    assert_eq!(cqes[1].user_data(), 0x02);
    assert_eq!(cqes[0].result(), 12);
    assert_eq!(cqes[1].result(), 12);

    assert_eq!(buf, TEXT);

    Ok(())
}
