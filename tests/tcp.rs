mod common;

use std::{ io, thread };
use std::mem::MaybeUninit;
use std::os::unix::io::AsRawFd;
use std::net::{ SocketAddr, TcpListener, TcpStream };
use lazy_static::lazy_static;
use linux_io_uring::opcode::{ self, types };
use linux_io_uring::{ squeue, reg, IoUring };
use common::do_write_read;


pub fn echo_server() -> &'static SocketAddr {
    lazy_static!{
        static ref TEST_SERVER: SocketAddr = {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let addr = listener.local_addr().unwrap();

            thread::spawn(move || {
                for stream in listener.incoming() {
                    let stream = stream.unwrap();
                    let (mut r, mut w) = (&stream, &stream);
                    io::copy(&mut r, &mut w).unwrap();
                }
            });

            addr
        };
    }

    &*TEST_SERVER
}

const TEXT: &[u8] = b"hello world!";

#[test]
fn test_tcp_write_then_read() -> anyhow::Result<()> {
    let addr = echo_server();
    let mut ring = IoUring::new(2)?;

    let stream = TcpStream::connect(addr)?;
    let fd = types::Target::Fd(stream.as_raw_fd());

    do_write_read(&mut ring, fd)?;

    Ok(())
}

#[test]
fn test_tcp_sendmsg_and_recvmsg() -> anyhow::Result<()> {
    let addr = echo_server();
    let sockaddr = socket2::SockAddr::from(*addr);
    let mut ring = IoUring::new(2)?;

    let stream = TcpStream::connect(addr)?;

    let buf = TEXT.to_vec();
    let mut buf2 = vec![0; TEXT.len()];
    let bufs = [io::IoSlice::new(&buf)];
    let mut bufs2 = [io::IoSliceMut::new(&mut buf2)];

    // reg fd
    ring.register(reg::Target::File(&[stream.as_raw_fd()]))?;

    // build sendmsg
    let mut msg = MaybeUninit::<libc::msghdr>::zeroed();

    unsafe {
        let p = msg.as_mut_ptr();
        (*p).msg_name = sockaddr.as_ptr() as *const _ as *mut _;
        (*p).msg_namelen = sockaddr.len();
        (*p).msg_iov = bufs.as_ptr() as *const _ as *mut _;
        (*p).msg_iovlen = 1;
    }

    let write_e = opcode::SendMsg::new(types::Target::Fixed(0), msg.as_ptr());

    // build recvmsg
    let mut msg = MaybeUninit::<libc::msghdr>::zeroed();

    unsafe {
        let p = msg.as_mut_ptr();
        (*p).msg_name = sockaddr.as_ptr() as *const _ as *mut _;
        (*p).msg_namelen = sockaddr.len();
        (*p).msg_iov = bufs2.as_mut_ptr() as *mut _;
        (*p).msg_iovlen = 1;
    }

    let read_e = opcode::RecvMsg::new(types::Target::Fixed(0), msg.as_mut_ptr());

    // submit
    unsafe {
        let mut queue = ring.submission().available();
        queue.push(write_e.build().user_data(0x01).flags(squeue::Flags::IO_LINK))
            .map_err(drop)
            .expect("queue is full");
        queue.push(read_e.build().user_data(0x02))
            .map_err(drop)
            .expect("queue is full");
    }

    ring.submit_and_wait(2)?;

    // complete
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
