use crate::helper;
use io_uring::{opcode, squeue, types, IoUring};
use once_cell::sync::OnceCell;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::io::AsRawFd;
use std::{io, mem, thread};

static ECHO_TCP_SERVER: OnceCell<SocketAddr> = OnceCell::new();

pub fn test_tcp_write_read(ring: &mut IoUring) -> anyhow::Result<()> {
    println!("test tcp_write_read");

    let addr = ECHO_TCP_SERVER.get_or_try_init(init_echo_tcp_server)?;

    let stream = TcpStream::connect(addr)?;
    let fd = types::Fd(stream.as_raw_fd());

    helper::write_read(ring, fd, fd)?;

    Ok(())
}

pub fn test_tcp_writev_readv(ring: &mut IoUring) -> anyhow::Result<()> {
    println!("test tcp_writev_readv");

    let addr = ECHO_TCP_SERVER.get_or_try_init(init_echo_tcp_server)?;

    let stream = TcpStream::connect(addr)?;
    let fd = types::Fd(stream.as_raw_fd());

    helper::writev_readv(ring, fd, fd)?;

    Ok(())
}

pub fn test_tcp_send_recv(ring: &mut IoUring) -> anyhow::Result<()> {
    println!("test tcp_send_recv");

    let addr = ECHO_TCP_SERVER.get_or_try_init(init_echo_tcp_server)?;

    let stream = TcpStream::connect(addr)?;
    let fd = types::Fd(stream.as_raw_fd());

    let text = b"The quick brown fox jumps over the lazy dog.";
    let mut output = vec![0; text.len()];

    let send_e = opcode::Send::new(fd, text.as_ptr(), text.len() as _);
    let recv_e = opcode::Recv::new(fd, output.as_mut_ptr(), output.len() as _);

    unsafe {
        let mut queue = ring.submission().available();
        let send_e = send_e.build().user_data(0x01).flags(squeue::Flags::IO_LINK);
        queue.push(send_e).ok().expect("queue is full");
        queue
            .push(recv_e.build().user_data(0x02))
            .ok()
            .expect("queue is full");
    }

    ring.submit_and_wait(2)?;

    let cqes = ring.completion().available().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].user_data(), 0x01);
    assert_eq!(cqes[1].user_data(), 0x02);
    assert_eq!(cqes[0].result(), text.len() as i32);
    assert_eq!(cqes[1].result(), text.len() as i32);

    assert_eq!(&output[..cqes[1].result() as usize], text);

    Ok(())
}

pub fn test_tcp_sendmsg_recvmsg(ring: &mut IoUring) -> anyhow::Result<()> {
    use std::mem::MaybeUninit;

    println!("test tcp_sendmsg_recvmsg");

    let addr = ECHO_TCP_SERVER.get_or_try_init(init_echo_tcp_server)?;

    let sockaddr = socket2::SockAddr::from(*addr);
    let stream = TcpStream::connect(addr)?;
    let fd = types::Fd(stream.as_raw_fd());

    let text = b"The quick brown fox jumps over the lazy dog.";
    let mut buf2 = vec![0; text.len()];
    let bufs = [io::IoSlice::new(text)];
    let mut bufs2 = [io::IoSliceMut::new(&mut buf2)];

    // build sendmsg
    let mut msg = MaybeUninit::<libc::msghdr>::zeroed();

    unsafe {
        let p = msg.as_mut_ptr();
        (*p).msg_name = sockaddr.as_ptr() as *const _ as *mut _;
        (*p).msg_namelen = sockaddr.len();
        (*p).msg_iov = bufs.as_ptr() as *const _ as *mut _;
        (*p).msg_iovlen = 1;
    }

    let sendmsg_e = opcode::SendMsg::new(fd, msg.as_ptr());

    // build recvmsg
    let mut msg = MaybeUninit::<libc::msghdr>::zeroed();

    unsafe {
        let p = msg.as_mut_ptr();
        (*p).msg_name = sockaddr.as_ptr() as *const _ as *mut _;
        (*p).msg_namelen = sockaddr.len();
        (*p).msg_iov = bufs2.as_mut_ptr() as *mut _;
        (*p).msg_iovlen = 1;
    }

    let recvmsg_e = opcode::RecvMsg::new(fd, msg.as_mut_ptr());

    // submit
    unsafe {
        let mut queue = ring.submission().available();
        queue
            .push(
                sendmsg_e
                    .build()
                    .user_data(0x01)
                    .flags(squeue::Flags::IO_LINK),
            )
            .ok()
            .expect("queue is full");
        queue
            .push(recvmsg_e.build().user_data(0x02))
            .ok()
            .expect("queue is full");
    }

    ring.submit_and_wait(2)?;

    // complete
    let cqes = ring.completion().available().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].user_data(), 0x01);
    assert_eq!(cqes[1].user_data(), 0x02);
    assert_eq!(cqes[0].result(), text.len() as _);
    assert_eq!(cqes[1].result(), text.len() as _);

    assert_eq!(buf2, text);

    Ok(())
}

pub fn test_tcp_accept(ring: &mut IoUring) -> anyhow::Result<()> {
    println!("test tcp_accept");

    let listener = TcpListener::bind("0.0.0.0:0")?;
    let addr = listener.local_addr()?;

    let handle = thread::spawn(move || {
        let stream = TcpStream::connect(addr)?;

        let mut stream2 = &stream;
        let mut stream3 = &stream;
        io::copy(&mut stream2, &mut stream3)
    });

    let fd = types::Fd(listener.as_raw_fd());

    let mut sockaddr: libc::sockaddr = unsafe { mem::zeroed() };
    let mut addrlen: libc::socklen_t = mem::size_of::<libc::sockaddr>() as _;

    let accept_e = opcode::Accept::new(fd, &mut sockaddr, &mut addrlen);

    unsafe {
        let mut queue = ring.submission().available();
        queue
            .push(accept_e.build().user_data(0x0e))
            .ok()
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes = ring.completion().available().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x0e);
    assert!(cqes[0].result() >= 0);

    let fd = cqes[0].result();

    helper::write_read(ring, types::Fd(fd), types::Fd(fd))?;

    unsafe {
        libc::close(fd);
    }

    handle.join().unwrap()?;

    Ok(())
}

pub fn test_tcp_connect(ring: &mut IoUring) -> anyhow::Result<()> {
    use socket2::{Domain, Protocol, SockAddr, Socket, Type};

    println!("test tcp_connect");

    let addr = ECHO_TCP_SERVER.get_or_try_init(init_echo_tcp_server)?;

    let sockaddr = SockAddr::from(*addr);
    let stream = Socket::new(Domain::ipv4(), Type::stream(), Some(Protocol::tcp()))?;

    let connect_e = opcode::Connect::new(
        types::Fd(stream.as_raw_fd()),
        sockaddr.as_ptr() as *const _,
        sockaddr.len(),
    );

    unsafe {
        let mut queue = ring.submission().available();
        queue
            .push(connect_e.build().user_data(0x0f))
            .ok()
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes = ring.completion().available().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x0f);
    assert_eq!(cqes[0].result(), 0);

    let stream = stream.into_tcp_stream();
    let fd = types::Fd(stream.as_raw_fd());

    helper::write_read(ring, fd, fd)?;

    Ok(())
}

fn init_echo_tcp_server() -> io::Result<SocketAddr> {
    let listener = TcpListener::bind("0.0.0.0:0")?;
    let addr = listener.local_addr()?;

    thread::spawn(move || {
        while let Ok((stream, _)) = listener.accept() {
            let mut stream2 = &stream;
            let mut stream3 = &stream;
            io::copy(&mut stream2, &mut stream3).unwrap();
        }
    });

    Ok(addr)
}
