mod common;

use std::{ io, thread };
use std::mem::MaybeUninit;
use std::os::unix::io::AsRawFd;
use std::net::{ SocketAddr, TcpListener, TcpStream };
use lazy_static::lazy_static;
use io_uring::opcode::{ self, types };
use io_uring::{ squeue, reg, IoUring };
use common::do_write_read;


pub fn echo_server() -> &'static SocketAddr {
    use std::mem;
    use std::os::unix::io::FromRawFd;

    lazy_static!{
        static ref TEST_SERVER: SocketAddr = {
            let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            let addr = listener.local_addr().unwrap();

            thread::spawn(move || {
                let mut ring = IoUring::new(1).unwrap();

                loop {
                    let mut sockaddr: libc::sockaddr = unsafe { mem::zeroed() };
                    let mut addrlen: libc::socklen_t = 0;

                    let accept_e = opcode::Accept::new(
                        types::Target::Fd(listener.as_raw_fd()),
                        &mut sockaddr,
                        &mut addrlen
                    );

                    unsafe {
                        ring.submission()
                            .available()
                            .push(accept_e.build())
                            .ok()
                            .expect("queue is full");
                    }

                    ring.submit_and_wait(1).unwrap();

                    let cqe = ring.completion()
                        .available()
                        .next()
                        .unwrap();

                    let stream = match cqe.result() {
                        ret if ret >= 0 => unsafe { TcpStream::from_raw_fd(ret) },
                        ret => panic!("accept failed: {:?}", io::Error::from_raw_os_error(-ret))
                    };

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
    ring.register(reg::Target::Files(&[stream.as_raw_fd()]))?;

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
            .ok()
            .expect("queue is full");
        queue.push(read_e.build().user_data(0x02))
            .ok()
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

#[test]
fn test_tcp_connect() -> anyhow::Result<()> {
    use socket2::{ SockAddr, Socket, Domain, Type, Protocol };

    let addr = echo_server();
    let sockaddr = SockAddr::from(*addr);
    let stream =
        Socket::new(Domain::ipv4(), Type::stream(), Some(Protocol::tcp()))?;
    let mut ring = IoUring::new(2)?;

    let connect_e = opcode::Connect::new(
        types::Target::Fd(stream.as_raw_fd()),
        sockaddr.as_ptr() as *const _,
        sockaddr.len()
    )
        .build()
        .user_data(0x42);

    unsafe {
        ring.submission()
            .available()
            .push(connect_e)
            .ok()
            .expect("squeue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe = ring.completion()
        .available()
        .next()
        .expect("cqueue is empty");

    assert_eq!(cqe.user_data(), 0x42);
    assert_eq!(cqe.result(), 0);

    let stream = stream.into_tcp_stream();

    do_write_read(&mut ring, types::Target::Fd(stream.as_raw_fd()))?;

    Ok(())
}
