use crate::utils;
use crate::Test;
use io_uring::{opcode, squeue, types, IoUring};
use once_cell::sync::OnceCell;
use std::net::{TcpListener, TcpStream};
use std::os::unix::io::AsRawFd;
use std::{io, mem};

static TCP_LISTENER: OnceCell<TcpListener> = OnceCell::new();

fn tcp_pair() -> io::Result<(TcpStream, TcpStream)> {
    let listener = TCP_LISTENER.get_or_try_init(|| TcpListener::bind("127.0.0.1:0"))?;

    let addr = listener.local_addr()?;
    let send_stream = TcpStream::connect(addr)?;
    let (recv_stream, _) = listener.accept()?;

    Ok((send_stream, recv_stream))
}

pub fn test_tcp_write_read(ring: &mut IoUring, test: &Test) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Write::CODE);
        test.probe.is_supported(opcode::Read::CODE);
    );

    println!("test tcp_write_read");

    let (send_stream, recv_stream) = tcp_pair()?;

    let send_fd = types::Fd(send_stream.as_raw_fd());
    let recv_fd = types::Fd(recv_stream.as_raw_fd());

    utils::write_read(ring, send_fd, recv_fd)?;

    Ok(())
}

pub fn test_tcp_writev_readv(ring: &mut IoUring, test: &Test) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Writev::CODE);
        test.probe.is_supported(opcode::Readv::CODE);
    );

    println!("test tcp_writev_readv");

    let (send_stream, recv_stream) = tcp_pair()?;

    let send_fd = types::Fd(send_stream.as_raw_fd());
    let recv_fd = types::Fd(recv_stream.as_raw_fd());

    utils::writev_readv(ring, send_fd, recv_fd)?;

    Ok(())
}

pub fn test_tcp_send_recv(ring: &mut IoUring, test: &Test) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Send::CODE);
        test.probe.is_supported(opcode::Recv::CODE);
    );

    println!("test tcp_send_recv");

    let (send_stream, recv_stream) = tcp_pair()?;

    let send_fd = types::Fd(send_stream.as_raw_fd());
    let recv_fd = types::Fd(recv_stream.as_raw_fd());

    let text = b"The quick brown fox jumps over the lazy dog.";
    let mut output = vec![0; text.len()];

    let send_e = opcode::Send::new(send_fd, text.as_ptr(), text.len() as _);
    let recv_e = opcode::Recv::new(recv_fd, output.as_mut_ptr(), output.len() as _);

    unsafe {
        let mut queue = ring.submission();
        let send_e = send_e.build().user_data(0x01).flags(squeue::Flags::IO_LINK);
        queue.push(&send_e).expect("queue is full");
        queue
            .push(&recv_e.build().user_data(0x02))
            .expect("queue is full");
    }

    ring.submit_and_wait(2)?;

    let cqes = ring.completion().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].user_data(), 0x01);
    assert_eq!(cqes[1].user_data(), 0x02);
    assert_eq!(cqes[0].result(), text.len() as i32);
    assert_eq!(cqes[1].result(), text.len() as i32);

    assert_eq!(&output[..cqes[1].result() as usize], text);

    Ok(())
}

pub fn test_tcp_sendmsg_recvmsg(ring: &mut IoUring, test: &Test) -> anyhow::Result<()> {
    use std::mem::MaybeUninit;

    require!(
        test;
        test.probe.is_supported(opcode::SendMsg::CODE);
        test.probe.is_supported(opcode::RecvMsg::CODE);
    );

    println!("test tcp_sendmsg_recvmsg");

    let (send_stream, recv_stream) = tcp_pair()?;

    let addr = recv_stream.local_addr()?;
    let sockaddr = socket2::SockAddr::from(addr);
    let send_fd = types::Fd(send_stream.as_raw_fd());
    let recv_fd = types::Fd(recv_stream.as_raw_fd());

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

    let sendmsg_e = opcode::SendMsg::new(send_fd, msg.as_ptr());

    // build recvmsg
    let mut msg = MaybeUninit::<libc::msghdr>::zeroed();

    unsafe {
        let p = msg.as_mut_ptr();
        (*p).msg_name = sockaddr.as_ptr() as *const _ as *mut _;
        (*p).msg_namelen = sockaddr.len();
        (*p).msg_iov = bufs2.as_mut_ptr() as *mut _;
        (*p).msg_iovlen = 1;
    }

    let recvmsg_e = opcode::RecvMsg::new(recv_fd, msg.as_mut_ptr());

    // submit
    unsafe {
        let mut queue = ring.submission();
        queue
            .push(
                &sendmsg_e
                    .build()
                    .user_data(0x01)
                    .flags(squeue::Flags::IO_LINK),
            )
            .expect("queue is full");
        queue
            .push(&recvmsg_e.build().user_data(0x02))
            .expect("queue is full");
    }

    ring.submit_and_wait(2)?;

    // complete
    let cqes = ring.completion().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].user_data(), 0x01);
    assert_eq!(cqes[1].user_data(), 0x02);
    assert_eq!(cqes[0].result(), text.len() as i32);
    assert_eq!(cqes[1].result(), text.len() as i32);

    assert_eq!(buf2, text);

    Ok(())
}

pub fn test_tcp_accept(ring: &mut IoUring, test: &Test) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Accept::CODE);
    );

    println!("test tcp_accept");

    let listener = TCP_LISTENER.get_or_try_init(|| TcpListener::bind("127.0.0.1:0"))?;
    let addr = listener.local_addr()?;
    let fd = types::Fd(listener.as_raw_fd());

    let _stream = TcpStream::connect(addr)?;

    let mut sockaddr: libc::sockaddr = unsafe { mem::zeroed() };
    let mut addrlen: libc::socklen_t = mem::size_of::<libc::sockaddr>() as _;

    let accept_e = opcode::Accept::new(fd, &mut sockaddr, &mut addrlen);

    unsafe {
        ring.submission()
            .push(&accept_e.build().user_data(0x0e))
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes = ring.completion().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x0e);
    assert!(cqes[0].result() >= 0);

    let fd = cqes[0].result();

    unsafe {
        libc::close(fd);
    }

    Ok(())
}

pub fn test_tcp_connect(ring: &mut IoUring, test: &Test) -> anyhow::Result<()> {
    use socket2::{Domain, Protocol, SockAddr, Socket, Type};

    require!(
        test;
        test.probe.is_supported(opcode::Connect::CODE);
    );

    println!("test tcp_connect");

    let listener = TCP_LISTENER.get_or_try_init(|| TcpListener::bind("127.0.0.1:0"))?;
    let addr = listener.local_addr()?;

    let sockaddr = SockAddr::from(addr);
    let stream = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;

    let connect_e = opcode::Connect::new(
        types::Fd(stream.as_raw_fd()),
        sockaddr.as_ptr() as *const _,
        sockaddr.len(),
    );

    unsafe {
        ring.submission()
            .push(&connect_e.build().user_data(0x0f))
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes = ring.completion().collect::<Vec<_>>();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x0f);
    assert_eq!(cqes[0].result(), 0);

    let _ = listener.accept()?;

    Ok(())
}

#[cfg(feature = "unstable")]
pub fn test_tcp_buffer_select(ring: &mut IoUring, test: &Test) -> anyhow::Result<()> {
    use io_uring::cqueue;
    use std::io::Write;

    require!(
        test;
        test.probe.is_supported(opcode::Send::CODE);
        test.probe.is_supported(opcode::Recv::CODE);
        test.probe.is_supported(opcode::ProvideBuffers::CODE);
        test.probe.is_supported(opcode::RemoveBuffers::CODE);
    );

    println!("test tcp_buffer_select");

    let (mut send_stream, recv_stream) = tcp_pair()?;

    let recv_fd = types::Fd(recv_stream.as_raw_fd());

    let mut input = vec![0xde; 1024];
    input.extend_from_slice(&[0xad; 256]);
    let mut bufs = vec![0; 1024];

    // provide bufs
    let provide_bufs_e = opcode::ProvideBuffers::new(bufs.as_mut_ptr(), 1024, 1, 0xdead, 0);

    unsafe {
        ring.submission()
            .push(&provide_bufs_e.build().user_data(0x21))
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe = ring.completion().next().expect("cqueue is empty");
    assert_eq!(cqe.user_data(), 0x21);
    // assert_eq!(cqe.result(), 0xdead);

    // write 1024 + 256
    send_stream.write_all(&input)?;

    // recv 1024
    let recv_e = opcode::Recv::new(recv_fd, std::ptr::null_mut(), 1024)
        .buf_group(0xdead)
        .build()
        .flags(squeue::Flags::BUFFER_SELECT)
        .user_data(0x22);

    unsafe {
        ring.submission().push(&recv_e).expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe = ring.completion().next().expect("cqueue is empty");
    assert_eq!(cqe.user_data(), 0x22);
    assert_eq!(cqe.result(), 1024);
    assert_eq!(cqueue::buffer_select(cqe.flags()), Some(0));
    assert_eq!(&bufs[..1024], &input[..1024]);

    // recv fail
    let recv_e = opcode::Recv::new(recv_fd, std::ptr::null_mut(), 1024)
        .buf_group(0xdead)
        .build()
        .flags(squeue::Flags::BUFFER_SELECT)
        .user_data(0x23);

    unsafe {
        ring.submission().push(&recv_e).expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe = ring.completion().next().expect("cqueue is empty");
    assert_eq!(cqe.user_data(), 0x23);
    assert_eq!(cqe.result(), -libc::ENOBUFS);

    // provides bufs 2
    bufs.extend_from_slice(&[0; 1024]);

    let provide_bufs_e = opcode::ProvideBuffers::new(bufs.as_mut_ptr(), 1024, 2, 0xdeae, 0);

    unsafe {
        ring.submission()
            .push(&provide_bufs_e.build().user_data(0x24))
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe = ring.completion().next().expect("cqueue is empty");
    assert_eq!(cqe.user_data(), 0x24);
    // assert_eq!(cqe.result(), 0xdeae);

    // recv 2
    let recv_e = opcode::Recv::new(recv_fd, std::ptr::null_mut(), 1024)
        .buf_group(0xdeae)
        .build()
        .flags(squeue::Flags::BUFFER_SELECT)
        .user_data(0x25);

    unsafe {
        ring.submission().push(&recv_e).expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe = ring.completion().next().expect("cqueue is empty");
    assert_eq!(cqe.user_data(), 0x25);
    assert_eq!(cqe.result(), 256);

    let (buf0, buf1) = bufs.split_at(1024);
    let bid = cqueue::buffer_select(cqe.flags()).expect("no buffer id");
    match bid {
        0 => assert_eq!(&buf0[..256], &input[1024..]),
        1 => assert_eq!(&buf1[..256], &input[1024..]),
        _ => panic!("{}", cqe.flags()),
    }

    // remove bufs
    let remove_bufs_e = opcode::RemoveBuffers::new(1, 0xdeae);

    unsafe {
        ring.submission()
            .push(&remove_bufs_e.build().user_data(0x26))
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe = ring.completion().next().expect("cqueue is empty");
    assert_eq!(cqe.user_data(), 0x26);
    assert_eq!(cqe.result(), 1);

    // remove bufs fail
    let remove_bufs_e = opcode::RemoveBuffers::new(1, 0xdeaf);

    unsafe {
        ring.submission()
            .push(&remove_bufs_e.build().user_data(0x27))
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe = ring.completion().next().expect("cqueue is empty");
    assert_eq!(cqe.user_data(), 0x27);
    assert_eq!(cqe.result(), -libc::ENOENT);

    Ok(())
}
