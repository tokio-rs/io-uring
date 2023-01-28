use crate::utils;
use crate::Test;
use io_uring::{cqueue, opcode, squeue, types, IoUring};
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

pub fn test_tcp_write_read<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
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

pub fn test_tcp_writev_readv<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
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

pub fn test_tcp_send_recv<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
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
        let send_e = send_e
            .build()
            .user_data(0x01)
            .flags(squeue::Flags::IO_LINK)
            .into();
        queue.push(&send_e).expect("queue is full");
        queue
            .push(&recv_e.build().user_data(0x02).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(2)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].user_data(), 0x01);
    assert_eq!(cqes[1].user_data(), 0x02);
    assert_eq!(cqes[0].result(), text.len() as i32);
    assert_eq!(cqes[1].result(), text.len() as i32);

    assert_eq!(&output[..cqes[1].result() as usize], text);

    Ok(())
}

pub fn test_tcp_zero_copy_send_recv<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::SendZc::CODE);
        test.probe.is_supported(opcode::Recv::CODE);
    );

    println!("test tcp_zero_copy_send_recv");

    let (send_stream, recv_stream) = tcp_pair()?;

    let send_fd = types::Fd(send_stream.as_raw_fd());
    let recv_fd = types::Fd(recv_stream.as_raw_fd());

    let text = b"The quick brown fox jumps over the lazy dog.";
    let mut output = vec![0; text.len()];

    let send_e = opcode::SendZc::new(send_fd, text.as_ptr(), text.len() as _);
    let recv_e = opcode::Recv::new(recv_fd, output.as_mut_ptr(), output.len() as _);

    unsafe {
        let mut queue = ring.submission();
        let send_e = send_e
            .build()
            .user_data(0x01)
            .flags(squeue::Flags::IO_LINK)
            .into();
        queue.push(&send_e).expect("queue is full");
        queue
            .push(&recv_e.build().user_data(0x02).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(2)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 3);
    // Send completion is ordered w.r.t recv
    assert_eq!(cqes[0].user_data(), 0x01);
    assert!(io_uring::cqueue::more(cqes[0].flags()));
    assert_eq!(cqes[0].result(), text.len() as i32);

    // Notification is not ordered w.r.t recv
    match (cqes[1].user_data(), cqes[2].user_data()) {
        (0x01, 0x02) => {
            assert!(!io_uring::cqueue::more(cqes[1].flags()));
            assert_eq!(cqes[2].result(), text.len() as i32);
            assert_eq!(&output[..cqes[2].result() as usize], text);
        }
        (0x02, 0x01) => {
            assert!(!io_uring::cqueue::more(cqes[2].flags()));
            assert_eq!(cqes[1].result(), text.len() as i32);
            assert_eq!(&output[..cqes[1].result() as usize], text);
        }
        _ => unreachable!(),
    }
    Ok(())
}

pub fn test_tcp_sendmsg_recvmsg<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
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
                    .flags(squeue::Flags::IO_LINK)
                    .into(),
            )
            .expect("queue is full");
        queue
            .push(&recvmsg_e.build().user_data(0x02).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(2)?;

    // complete
    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 2);
    assert_eq!(cqes[0].user_data(), 0x01);
    assert_eq!(cqes[1].user_data(), 0x02);
    assert_eq!(cqes[0].result(), text.len() as i32);
    assert_eq!(cqes[1].result(), text.len() as i32);

    assert_eq!(buf2, text);

    Ok(())
}

pub fn test_tcp_zero_copy_sendmsg_recvmsg<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    use std::mem::MaybeUninit;

    require!(
        test;
        test.probe.is_supported(opcode::SendMsgZc::CODE);
        test.probe.is_supported(opcode::RecvMsg::CODE);
    );

    println!("test_tcp_zero_copy_sendmsg_recvmsg");

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

    let sendmsg_e = opcode::SendMsgZc::new(send_fd, msg.as_ptr());

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
                    .flags(squeue::Flags::IO_LINK)
                    .into(),
            )
            .expect("queue is full");
        queue
            .push(&recvmsg_e.build().user_data(0x02).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(2)?;

    // complete
    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 3);

    // Send completion is ordered w.r.t recv
    assert_eq!(cqes[0].user_data(), 0x01);
    assert!(io_uring::cqueue::more(cqes[0].flags()));
    assert_eq!(cqes[0].result(), text.len() as i32);

    // Notification is not ordered w.r.t recv
    match (cqes[1].user_data(), cqes[2].user_data()) {
        (0x01, 0x02) => {
            assert!(!io_uring::cqueue::more(cqes[1].flags()));
            assert_eq!(cqes[2].result(), text.len() as i32);
            assert_eq!(&buf2[..cqes[2].result() as usize], text);
        }
        (0x02, 0x01) => {
            assert!(!io_uring::cqueue::more(cqes[2].flags()));
            assert_eq!(cqes[1].result(), text.len() as i32);
            assert_eq!(&buf2[..cqes[1].result() as usize], text);
        }
        _ => unreachable!(),
    }
    Ok(())
}

pub fn test_tcp_accept<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
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
            .push(&accept_e.build().user_data(0x0e).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x0e);
    assert!(cqes[0].result() >= 0);

    let fd = cqes[0].result();

    unsafe {
        libc::close(fd);
    }

    Ok(())
}

pub fn test_tcp_connect<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
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
            .push(&connect_e.build().user_data(0x0f).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x0f);
    assert_eq!(cqes[0].result(), 0);

    let _ = listener.accept()?;

    Ok(())
}

pub fn test_tcp_buffer_select<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
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
            .push(&provide_bufs_e.build().user_data(0x21).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe: cqueue::Entry = ring.completion().next().expect("cqueue is empty").into();
    assert_eq!(cqe.user_data(), 0x21);
    // assert_eq!(cqe.result(), 0xdead);

    // write 1024 + 256
    send_stream.write_all(&input)?;

    // recv 1024
    let recv_e = opcode::Recv::new(recv_fd, std::ptr::null_mut(), 1024)
        .buf_group(0xdead)
        .build()
        .flags(squeue::Flags::BUFFER_SELECT)
        .user_data(0x22)
        .into();

    unsafe {
        ring.submission().push(&recv_e).expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe: cqueue::Entry = ring.completion().next().expect("cqueue is empty").into();
    assert_eq!(cqe.user_data(), 0x22);
    assert_eq!(cqe.result(), 1024);
    assert_eq!(cqueue::buffer_select(cqe.flags()), Some(0));
    assert_eq!(&bufs[..1024], &input[..1024]);

    // recv fail
    let recv_e = opcode::Recv::new(recv_fd, std::ptr::null_mut(), 1024)
        .buf_group(0xdead)
        .build()
        .flags(squeue::Flags::BUFFER_SELECT)
        .user_data(0x23)
        .into();

    unsafe {
        ring.submission().push(&recv_e).expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe: cqueue::Entry = ring.completion().next().expect("cqueue is empty").into();
    assert_eq!(cqe.user_data(), 0x23);
    assert_eq!(cqe.result(), -libc::ENOBUFS);

    // provides two bufs, one of which we will use, one we will free
    let mut bufs = vec![0; 2 * 1024];

    let provide_bufs_e = opcode::ProvideBuffers::new(bufs.as_mut_ptr(), 1024, 2, 0xdeae, 0);

    unsafe {
        ring.submission()
            .push(&provide_bufs_e.build().user_data(0x24).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe: cqueue::Entry = ring.completion().next().expect("cqueue is empty").into();
    assert_eq!(cqe.user_data(), 0x24);
    // assert_eq!(cqe.result(), 0xdeae);

    // recv 2
    let recv_e = opcode::Recv::new(recv_fd, std::ptr::null_mut(), 1024)
        .buf_group(0xdeae)
        .build()
        .flags(squeue::Flags::BUFFER_SELECT)
        .user_data(0x25)
        .into();

    unsafe {
        ring.submission().push(&recv_e).expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe: cqueue::Entry = ring.completion().next().expect("cqueue is empty").into();
    assert_eq!(cqe.user_data(), 0x25);
    assert_eq!(cqe.result(), 256);

    let (buf0, buf1) = bufs.split_at(1024);
    let bid = cqueue::buffer_select(cqe.flags()).expect("no buffer id");
    match bid {
        0 => assert_eq!(&buf0[..256], &input[1024..]),
        1 => assert_eq!(&buf1[..256], &input[1024..]),
        _ => panic!("{}", cqe.flags()),
    }

    // remove one remaining buf
    let remove_bufs_e = opcode::RemoveBuffers::new(1, 0xdeae);

    unsafe {
        ring.submission()
            .push(&remove_bufs_e.build().user_data(0x26).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe: cqueue::Entry = ring.completion().next().expect("cqueue is empty").into();
    assert_eq!(cqe.user_data(), 0x26);
    assert_eq!(cqe.result(), 1);

    // remove bufs fail
    let remove_bufs_e = opcode::RemoveBuffers::new(1, 0xdeaf);

    unsafe {
        ring.submission()
            .push(&remove_bufs_e.build().user_data(0x27).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe: cqueue::Entry = ring.completion().next().expect("cqueue is empty").into();
    assert_eq!(cqe.user_data(), 0x27);
    assert_eq!(cqe.result(), -libc::ENOENT);

    Ok(())
}

pub fn test_tcp_buffer_select_recvmsg<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    use std::io::Write;

    require!(
        test;
        test.probe.is_supported(opcode::RecvMsg::CODE);
        test.probe.is_supported(opcode::Readv::CODE);
        test.probe.is_supported(opcode::ProvideBuffers::CODE);
        test.probe.is_supported(opcode::RemoveBuffers::CODE);
    );

    println!("test tcp_buffer_select_recvmsg");

    let (mut send_stream, recv_stream) = tcp_pair()?;

    let recv_fd = types::Fd(recv_stream.as_raw_fd());

    const BGID: u16 = 0xdeaf;
    const INPUT_BID: u16 = 100;

    // provide two buffers for recvmsg
    let mut buf = [0u8; 2 * 1024];
    let provide_bufs_e = opcode::ProvideBuffers::new(buf.as_mut_ptr(), 1024, 2, BGID, INPUT_BID);

    unsafe {
        ring.submission()
            .push(&provide_bufs_e.build().user_data(0x26).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe: cqueue::Entry = ring.completion().next().expect("cqueue is empty").into();
    assert_eq!(cqe.user_data(), 0x26);

    // send for recvmsg
    send_stream.write_all(&[0x56u8; 1024])?;
    send_stream.write_all(&[0x57u8; 1024])?;

    // recvmsg
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    let mut iovecs: [libc::iovec; 1] = unsafe { std::mem::zeroed() };
    iovecs[0].iov_len = 1024; // This can be used to reduce the length of the read.
    msg.msg_iov = &mut iovecs as *mut _;
    msg.msg_iovlen = 1; // 2 results in EINVAL, Invalid argument, being returned in result.

    // N.B. This op will only support a BUFFER_SELECT when the msg.msg_iovlen is 1;
    // the kernel will return EINVAL for anything else. There would be no way of knowing
    // which other buffer IDs had been chosen.
    let op = opcode::RecvMsg::new(recv_fd, &mut msg as *mut _)
        .buf_group(BGID) // else result is -105, ENOBUFS, no buffer space available
        .build()
        .flags(squeue::Flags::BUFFER_SELECT) // else result is -14, EFAULT, bad address
        .user_data(0x27);

    // Safety: the msghdr and the iovecs remain valid for length of the operation.
    unsafe {
        ring.submission().push(&op.into()).expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe: cqueue::Entry = ring.completion().next().expect("cqueue is empty").into();
    assert_eq!(cqe.user_data(), 0x27);
    assert_eq!(cqe.result(), 1024); // -14 would mean EFAULT, bad address.

    let bid = cqueue::buffer_select(cqe.flags()).expect("no buffer id");
    if bid == INPUT_BID {
        // The 6.1 case.
        // Test buffer slice associated with the given bid.
        assert_eq!(&(buf[..1024]), &([0x56u8; 1024][..]));
    } else if bid == (INPUT_BID + 1) {
        // The 5.15 case.
        // Test buffer slice associated with the given bid.
        assert_eq!(&(buf[1024..]), &([0x56u8; 1024][..]));
    } else {
        panic!(
            "cqe bid {}, was neither {} nor {}",
            bid,
            INPUT_BID,
            INPUT_BID + 1
        );
    }

    Ok(())
}

pub fn test_tcp_buffer_select_readv<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    use std::io::Write;

    require!(
        test;
        test.probe.is_supported(opcode::RecvMsg::CODE);
        test.probe.is_supported(opcode::Readv::CODE);
        test.probe.is_supported(opcode::ProvideBuffers::CODE);
        test.probe.is_supported(opcode::RemoveBuffers::CODE);
    );

    println!("test tcp_buffer_select_readv");

    let (mut send_stream, recv_stream) = tcp_pair()?;

    let recv_fd = types::Fd(recv_stream.as_raw_fd());

    const BGID: u16 = 0xdeb0;
    const INPUT_BID: u16 = 200;

    // provide buf for readv
    let mut buf = [0u8; 512];
    let provide_bufs_e = opcode::ProvideBuffers::new(buf.as_mut_ptr(), 512, 1, BGID, INPUT_BID);

    unsafe {
        ring.submission()
            .push(&provide_bufs_e.build().user_data(0x29).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe: cqueue::Entry = ring.completion().next().expect("cqueue is empty").into();
    assert_eq!(cqe.user_data(), 0x29);

    // send for readv
    send_stream.write_all(&[0x7bu8; 512])?;

    let iovec = libc::iovec {
        iov_base: std::ptr::null_mut(),
        iov_len: 512, // Bug in earlier kernels requires this length to be the buffer pool
                      // length. By 6.1, this could be passed in as zero.
    };

    // readv
    // N.B. This op will only support a BUFFER_SELECT when the iovec length is 1,
    // the kernel should return EINVAL for anything else.
    let op = opcode::Readv::new(recv_fd, &iovec, 1)
        .buf_group(BGID)
        .build()
        .flags(squeue::Flags::BUFFER_SELECT)
        .user_data(0x2a);

    // Safety: The iovec addressed by the `op` remains live through the `submit_and_wait` call.
    unsafe {
        ring.submission().push(&op.into()).expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe: cqueue::Entry = ring.completion().next().expect("cqueue is empty").into();
    assert_eq!(cqe.user_data(), 0x2a);
    assert_eq!(cqe.result(), 512);

    let bid = cqueue::buffer_select(cqe.flags()).expect("no buffer id");
    assert_eq!(bid, INPUT_BID);
    // Test with buffer associated with the INPUT_BID.
    assert_eq!(&(buf[..]), &([0x7bu8; 512][..]));

    Ok(())
}
