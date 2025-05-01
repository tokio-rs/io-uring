use crate::tests::register_buf_ring;
use crate::utils;
use crate::Test;
use io_uring::squeue::Flags;
use io_uring::types::{BufRingEntry, Fd};
use io_uring::{cqueue, opcode, squeue, types, IoUring};
use once_cell::sync::OnceCell;
use std::convert::TryInto;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::fd::FromRawFd;
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

pub fn test_tcp_send_bundle<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::SendBundle::CODE);
        ring.params().is_feature_recvsend_bundle(); // requires 6.10
    );

    println!("test tcp_send_bundle");

    let (send_stream, mut recv_stream) = tcp_pair()?;

    let send_fd = types::Fd(send_stream.as_raw_fd());

    let text = b"The quick brown fox jumps over the lazy dog.";
    let mut output = vec![0; text.len()];

    let buf_ring = register_buf_ring::Builder::new(0xdead)
        .ring_entries(2)
        .buf_cnt(2)
        .buf_len(22)
        .build()?;
    buf_ring.rc.register(ring)?;
    let ptr1 = buf_ring.rc.ring_start.as_ptr_mut() as *mut BufRingEntry;
    unsafe {
        let ptr2 = ptr1.add(1);
        std::ptr::copy_nonoverlapping(text.as_ptr(), ptr1.as_mut().unwrap().addr() as *mut u8, 22);
        std::ptr::copy_nonoverlapping(
            text[22..].as_ptr(),
            ptr2.as_mut().unwrap().addr() as *mut u8,
            22,
        );
    }

    let send_e = opcode::SendBundle::new(send_fd, 0xdead);

    unsafe {
        let mut queue = ring.submission();
        let send_e = send_e.build().user_data(0x01).into();
        queue.push(&send_e).expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x01);
    assert_eq!(cqes[0].result(), text.len() as i32);

    assert_eq!(
        recv_stream
            .read(&mut output)
            .expect("could not read stream"),
        text.len()
    );
    assert_eq!(&output, text);
    buf_ring.rc.unregister(ring)?;

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

pub fn test_tcp_zero_copy_send_fixed<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::SendZc::CODE);
        test.probe.is_supported(opcode::Recv::CODE);
    );

    println!("test tcp_zero_copy_send_fixed");

    let (send_stream, recv_stream) = tcp_pair()?;

    let send_fd = types::Fd(send_stream.as_raw_fd());
    let recv_fd = types::Fd(recv_stream.as_raw_fd());

    let text = b"The quick brown fox jumps over the lazy dog.";
    let mut output = vec![0; text.len()];

    // Cleanup all fixed buffers (if any), then register one, with id 0.
    let _ = ring.submitter().unregister_buffers();

    let mut buf0 = vec![0; 1024];
    let iovec = libc::iovec {
        iov_base: buf0.as_ptr() as _,
        iov_len: buf0.len() as _,
    };
    let iovecs = [iovec];
    unsafe { ring.submitter().register_buffers(&iovecs).unwrap() };

    let text_len = text.len();

    // This works but is boring, sending data from the head of the registered buffer.
    // buf0[..text_len].copy_from_slice(&text[..]);
    // let send_e = opcode::SendZc::new(send_fd, buf0.as_ptr(), text_len as _).buf_index(Some(0));

    // Here, send data from the seventh position of the registered buffer.
    buf0[7..(text_len + 7)].copy_from_slice(&text[..]);
    let send_e = opcode::SendZc::new(send_fd, buf0[7..].as_ptr(), text_len as _).buf_index(Some(0));

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

    let _ = ring.submitter().unregister_buffers();

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

pub fn test_tcp_accept_file_index<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Accept::CODE);
        test.probe.is_supported(opcode::Socket::CODE); // To get file_index
    );

    println!("test tcp_accept_file_index");

    // Cleanup all fixed files (if any), then reserve five slots 0..=4.
    let _ = ring.submitter().unregister_files();
    ring.submitter().register_files_sparse(5).unwrap();

    // begin <from accept unit test>

    let listener = TCP_LISTENER.get_or_try_init(|| TcpListener::bind("127.0.0.1:0"))?;
    let addr = listener.local_addr()?;
    let fd = types::Fd(listener.as_raw_fd());

    let _stream = TcpStream::connect(addr)?;

    let mut sockaddr: libc::sockaddr = unsafe { mem::zeroed() };
    let mut addrlen: libc::socklen_t = mem::size_of::<libc::sockaddr>() as _;

    let dest_slot = types::DestinationSlot::try_from_slot_target(4).unwrap();
    let accept_e = opcode::Accept::new(fd, &mut sockaddr, &mut addrlen);
    let accept_e = accept_e.file_index(Some(dest_slot));

    unsafe {
        ring.submission()
            .push(&accept_e.build().user_data(0x0e).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x0e);
    assert_eq!(cqes[0].result(), 0); // success iff result is zero.

    // end of <from accept unit test>

    // The new tcp stream socket, with fixed descriptor at [4], will be released when the files are
    // unregistered.

    // If the fixed-socket operation worked properly, this must not fail.
    ring.submitter().unregister_files().unwrap();

    Ok(())
}

pub fn test_tcp_accept_multi<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Accept::CODE);
        test.probe.is_supported(opcode::Socket::CODE); // check 5.19 kernel
    );

    println!("test tcp_accept_multi");

    let listener = TCP_LISTENER.get_or_try_init(|| TcpListener::bind("127.0.0.1:0"))?;
    let addr = listener.local_addr()?;
    let fd = types::Fd(listener.as_raw_fd());

    // 2 streams

    let _stream1 = TcpStream::connect(addr)?;
    let _stream2 = TcpStream::connect(addr)?;

    let accept_e = opcode::AcceptMulti::new(fd);

    unsafe {
        ring.submission()
            .push(&accept_e.build().user_data(2002).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(2)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 2);

    for cqe in cqes {
        assert_eq!(cqe.user_data(), 2002);
        assert!(cqe.result() >= 0);

        let fd = cqe.result();

        unsafe {
            libc::close(fd);
        }
    }

    // Cancel the multishot accept

    let cancel_e = opcode::AsyncCancel::new(2002);

    unsafe {
        ring.submission()
            .push(&cancel_e.build().user_data(2003).into())
            .expect("queue is full");
    }

    // Wait for 2, the one canceled, and the one doing the cancel.

    ring.submit_and_wait(2)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 2);

    let (op1, op2) = match cqes[0].user_data() {
        2002 => (0, 1),
        _ => (1, 0),
    };
    assert_eq!(cqes[op1].user_data(), 2002);
    assert_eq!(cqes[op2].user_data(), 2003);

    assert_eq!(cqes[op1].result(), -125); // -ECANCELED
    assert_eq!(cqes[op2].result(), 0);

    Ok(())
}

pub fn test_tcp_accept_multi_file_index<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Accept::CODE);
        test.probe.is_supported(opcode::Socket::CODE); // check 5.19 kernel
    );

    println!("test tcp_accept_multi_file_index");

    let listener = TCP_LISTENER.get_or_try_init(|| TcpListener::bind("127.0.0.1:0"))?;
    let addr = listener.local_addr()?;
    let fd = types::Fd(listener.as_raw_fd());

    // 2 streams

    let _stream1 = TcpStream::connect(addr)?;
    let _stream2 = TcpStream::connect(addr)?;

    // 2 fixed table index spots

    // Cleanup all fixed files (if any), then reserve slot 0.
    let _ = ring.submitter().unregister_files();

    ring.submitter().register_files_sparse(2).unwrap();
    let accept_e = opcode::AcceptMulti::new(fd).allocate_file_index(true);

    unsafe {
        ring.submission()
            .push(&accept_e.build().user_data(2002).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(2)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 2);
    #[allow(clippy::needless_range_loop)]
    for round in 0..=1 {
        assert_eq!(cqes[round].user_data(), 2002);
        assert!(cqes[round].result() >= 0);

        // The fixed descriptor will be closed when the
        // table is unregistered below.
    }

    // Cancel the multishot accept

    let cancel_e = opcode::AsyncCancel::new(2002);

    unsafe {
        ring.submission()
            .push(&cancel_e.build().user_data(2003).into())
            .expect("queue is full");
    }

    // Wait for 2, the one canceled, and the one doing the cancel.

    ring.submit_and_wait(2)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 2);

    // Don't want to hardcode which one is returned first.
    let (op1, op2) = match cqes[0].user_data() {
        2002 => (0, 1),
        _ => (1, 0),
    };
    assert_eq!(cqes[op1].user_data(), 2002);
    assert_eq!(cqes[op2].user_data(), 2003);

    assert_eq!(cqes[op1].result(), -125); // -ECANCELED
    assert_eq!(cqes[op2].result(), 0);

    // If the fixed-socket operation worked properly, this must not fail.
    ring.submitter().unregister_files().unwrap();

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

pub fn test_tcp_recv_multi<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    use std::io::Write;

    require!(
        test;
        test.probe.is_supported(opcode::Send::CODE);
        test.probe.is_supported(opcode::Recv::CODE);
        test.probe.is_supported(opcode::SendZc::CODE); // also available 6.0, like the multishot for recv
        test.probe.is_supported(opcode::ProvideBuffers::CODE);
        test.probe.is_supported(opcode::RemoveBuffers::CODE);
    );

    println!("test tcp_recv_multi");

    let (mut send_stream, recv_stream) = tcp_pair()?;

    let recv_fd = types::Fd(recv_stream.as_raw_fd());

    // Send one package made of two segments, and receive as two buffers, each max length 1024
    // so the first buffer received should be length 1024 and the second length 256.
    let mut input = vec![0xde; 1024];
    input.extend_from_slice(&[0xad; 256]);
    let mut bufs = vec![0; 2 * 1024];

    // provide bufs
    let provide_bufs_e = opcode::ProvideBuffers::new(bufs.as_mut_ptr(), 1024, 2, 0xdead, 0);

    unsafe {
        ring.submission()
            .push(&provide_bufs_e.build().user_data(0x21).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqe: cqueue::Entry = ring.completion().next().expect("cqueue is empty").into();
    assert_eq!(cqe.user_data(), 0x21);
    assert_eq!(cqe.result(), 0);

    // write all 1024 + 256
    send_stream.write_all(&input)?;
    send_stream.shutdown(Shutdown::Write)?;

    // multishot recv using a buf_group with 1024 length buffers
    let recv_e = opcode::RecvMulti::new(recv_fd, 0xdead)
        .build()
        .user_data(0x22)
        .into();

    unsafe {
        ring.submission().push(&recv_e).expect("queue is full");
    }

    ring.submit_and_wait(3)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
    assert_eq!(cqes.len(), 3);

    assert_eq!(cqes[0].user_data(), 0x22);
    assert_eq!(cqes[0].result(), 1024); // length 1024
    assert!(cqueue::more(cqes[0].flags()));
    assert_eq!(cqueue::buffer_select(cqes[0].flags()), Some(0));
    assert_eq!(&bufs[..1024], &input[..1024]);

    assert_eq!(cqes[1].user_data(), 0x22);
    assert_eq!(cqes[1].result(), 256); // length 256
    assert!(cqueue::more(cqes[1].flags()));
    assert_eq!(cqueue::buffer_select(cqes[1].flags()), Some(1));
    assert_eq!(&bufs[1024..][..256], &input[1024..][..256]);

    assert_eq!(cqes[2].user_data(), 0x22);
    assert!(!cqueue::more(cqes[2].flags()));
    assert_eq!(cqes[2].result(), -105); // No buffer space available

    Ok(())
}

pub fn test_tcp_recv_bundle<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    use std::io::Write;

    require!(
        test;
        test.probe.is_supported(opcode::RecvBundle::CODE);
        ring.params().is_feature_recvsend_bundle(); // requires 6.10
    );

    println!("test tcp_recv_bundle");

    let (mut send_stream, recv_stream) = tcp_pair()?;

    let recv_fd = types::Fd(recv_stream.as_raw_fd());

    // Send one package made of four segments, and receive as up to two buffer bundles
    let mut input = vec![0x0d; 256];
    input.extend_from_slice(&[0x0e; 256]);
    input.extend_from_slice(&[0x0a; 256]);
    input.extend_from_slice(&[0x0d; 128]);

    // Prepare BufRing
    let buf_ring = register_buf_ring::Builder::new(0xdeff)
        .ring_entries(16)
        .buf_cnt(32)
        .buf_len(256)
        .build()?;
    buf_ring.rc.register(ring)?;

    send_stream.write_all(&input)?;
    send_stream.shutdown(Shutdown::Write)?;

    let mut input = input.as_slice();

    // Multiple receive operations might be needed to receive everything.
    loop {
        let recv_e = opcode::RecvBundle::new(recv_fd, 0xdeff)
            .build()
            .user_data(0x30)
            .into();

        unsafe {
            ring.submission().push(&recv_e).expect("queue is full");
        }

        ring.submit_and_wait(1)?;

        let cqe: cqueue::Entry = ring.completion().next().expect("cqueue is empty").into();

        assert_eq!(cqe.user_data(), 0x30);
        assert!(cqueue::buffer_select(cqe.flags()).is_some());
        let mut remaining = cqe.result() as usize;
        let bufs = buf_ring
            .rc
            .get_bufs(&buf_ring, remaining as u32, cqe.flags());
        let mut section;
        for buf in &bufs {
            let to_check = std::cmp::min(256, remaining);
            (section, input) = input.split_at(to_check);
            assert_eq!(buf.as_slice(), section);
            remaining -= to_check;
        }
        assert_eq!(remaining, 0);

        if input.is_empty() {
            break;
        }

        assert!(cqueue::sock_nonempty(cqe.flags()));
    }

    buf_ring.rc.unregister(ring)?;

    Ok(())
}

pub fn test_tcp_recv_multi_bundle<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::RecvMultiBundle::CODE);
        ring.params().is_feature_recvsend_bundle(); // requires 6.10
    );

    println!("test tcp_recv_multi_bundle");

    let (mut send_stream, recv_stream) = tcp_pair()?;

    let recv_fd = types::Fd(recv_stream.as_raw_fd());

    // Send one package made of four segments, and receive as up to two buffer bundles
    let mut input = vec![0x0d; 256];
    input.extend_from_slice(&[0x0e; 256]);
    input.extend_from_slice(&[0x0a; 256]);
    input.extend_from_slice(&[0x0d; 128]);

    // Prepare BufRing
    let buf_ring = register_buf_ring::Builder::new(0xdebf)
        .ring_entries(2)
        .buf_cnt(5)
        .buf_len(256)
        .build()?;
    buf_ring.rc.register(ring)?;

    send_stream.write_all(&input)?;
    send_stream.shutdown(Shutdown::Write)?;

    let recv_e = opcode::RecvMultiBundle::new(recv_fd, 0xdebf)
        .build()
        .user_data(0x31)
        .into();

    unsafe {
        ring.submission().push(&recv_e).expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let mut cqe: cqueue::Entry = ring.completion().next().expect("cqueue is empty").into();

    assert!(cqe.result() >= 0);
    assert_eq!(cqe.user_data(), 0x31);
    assert!(cqueue::buffer_select(cqe.flags()).is_some());
    let mut remaining = cqe.result() as usize;
    let bufs = buf_ring
        .rc
        .get_bufs(&buf_ring, remaining as u32, cqe.flags());
    let mut section;
    let mut input = input.as_slice();
    for buf in &bufs {
        // In case of bundled recv first bundle may not be full
        let to_check = std::cmp::min(256, remaining);
        (section, input) = input.split_at(to_check);
        assert_eq!(buf.as_slice(), section);
        remaining -= to_check;
    }
    assert_eq!(remaining, 0);

    let mut used_bufs = bufs.len();

    // Linux kernel 6.10 packs a single buffer into first recv and remaining buffers into second recv
    // This behavior may change in the future
    if !input.is_empty() {
        assert!(cqueue::more(cqe.flags()));

        ring.submit_and_wait(1)?;

        cqe = ring.completion().next().expect("cqueue is empty").into();

        assert!(cqe.result() >= 0);
        assert_eq!(cqe.user_data(), 0x31);
        assert!(cqueue::buffer_select(cqe.flags()).is_some());
        remaining = cqe.result() as usize;
        let second_bufs = buf_ring
            .rc
            .get_bufs(&buf_ring, remaining as u32, cqe.flags());
        for buf in &second_bufs {
            let to_check = std::cmp::min(256, remaining);
            (section, input) = input.split_at(to_check);
            assert_eq!(buf.as_slice(), section);
            remaining -= to_check;
        }
        assert_eq!(remaining, 0);
        used_bufs += second_bufs.len();
    }
    assert!(input.is_empty());

    if cqueue::more(cqe.flags()) {
        ring.submit_and_wait(1)?;
        cqe = ring.completion().next().expect("cqueue is empty").into();
        assert_eq!(cqe.user_data(), 0x31);
        assert!(!cqueue::more(cqe.flags()));
        if used_bufs >= 5 || cqe.result() != 0 {
            assert_eq!(cqe.result(), -105); // No buffer space available
        }
    }
    buf_ring.rc.unregister(ring)?;

    Ok(())
}

pub fn test_tcp_shutdown<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Write::CODE);
        test.probe.is_supported(opcode::Shutdown::CODE);
    );

    println!("test tcp_shutdown");

    const SHUT_WR: i32 = 1;

    let listener = TCP_LISTENER.get_or_try_init(|| TcpListener::bind("127.0.0.1:0"))?;
    let sock_fd = types::Fd(listener.as_raw_fd());

    let shutdown_e = opcode::Shutdown::new(sock_fd, SHUT_WR);

    unsafe {
        ring.submission()
            .push(&shutdown_e.build().user_data(0x28).into())
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 0x28);
    assert_eq!(cqes[0].result(), 0);

    let text = b"C'est la vie";
    let write_e = opcode::Write::new(sock_fd, text.as_ptr(), text.len() as _);

    unsafe {
        ring.submission()
            .push(&write_e.build().into())
            .expect("queue is full");
    }

    ring.submit_and_wait(1)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].result(), -32); // EPIPE

    Ok(())
}

pub fn test_socket<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    use socket2::{Domain, Protocol, Socket, Type};

    require!(
        test;
        test.probe.is_supported(opcode::Socket::CODE);
        test.probe.is_supported(opcode::Bind::CODE);
    );

    println!("test socket");

    // Open a UDP socket, through old-style `socket(2)` syscall.
    // This is used both as a kernel sanity check, and for comparing the returned io-uring FD.
    let plain_socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)).unwrap();
    let plain_fd = plain_socket.as_raw_fd();

    let socket_fd_op = opcode::Socket::new(
        Domain::IPV4.into(),
        Type::STREAM.into(),
        Protocol::TCP.into(),
    );
    unsafe {
        ring.submission()
            .push(&socket_fd_op.build().user_data(42).into())
            .expect("queue is full");
    }
    ring.submit_and_wait(1)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 42);
    assert!(cqes[0].result() >= 0);
    let io_uring_socket = unsafe { Socket::from_raw_fd(cqes[0].result()) };
    assert!(io_uring_socket.as_raw_fd() != plain_fd);
    assert_eq!(cqes[0].flags(), 0);

    // Try a setsockopt.
    {
        let mut optval: libc::c_int = 0;
        let mut optval_size: libc::socklen_t = std::mem::size_of_val(&optval) as libc::socklen_t;
        // Get value before.
        let ret = unsafe {
            libc::getsockopt(
                io_uring_socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_REUSEADDR,
                &mut optval as *mut _ as *mut libc::c_void,
                &mut optval_size as *mut _ as *mut libc::socklen_t,
            )
        };
        assert_eq!(ret, 0);
        assert_eq!(optval, 0);

        // Set value.
        optval = 1;
        let op = io_uring::opcode::SetSockOpt::new(
            io_uring::types::Fd(io_uring_socket.as_raw_fd()),
            libc::SOL_SOCKET as u32,
            libc::SO_REUSEADDR as u32,
            &optval as *const _ as *const libc::c_void,
            std::mem::size_of_val(&optval) as libc::socklen_t,
        )
        .build()
        .user_data(1234);
        unsafe {
            ring.submission().push(&op.into()).expect("queue is full");
        }
        ring.submit_and_wait(1)?;
        let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
        assert_eq!(cqes.len(), 1);
        assert_eq!(cqes[0].user_data(), 1234);
        assert_eq!(cqes[0].result(), 0);
        assert_eq!(cqes[0].flags(), 0);

        // Check value actually set.
        optval = 0;
        let ret = unsafe {
            libc::getsockopt(
                io_uring_socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_REUSEADDR,
                &mut optval as *mut _ as *mut libc::c_void,
                &mut optval_size as *mut _ as *mut libc::socklen_t,
            )
        };
        assert_eq!(ret, 0);
        assert_eq!(optval, 1);
    }

    // Try to bind.
    {
        let server_addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let server_addr: socket2::SockAddr = server_addr.into();
        let op = io_uring::opcode::Bind::new(
            io_uring::types::Fd(io_uring_socket.as_raw_fd()),
            server_addr.as_ptr() as *const _,
            server_addr.len(),
        )
        .build()
        .user_data(2345);
        unsafe {
            ring.submission().push(&op.into()).expect("queue is full");
        }
        ring.submit_and_wait(1)?;
        let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
        assert_eq!(cqes.len(), 1);
        assert_eq!(cqes[0].user_data(), 2345);
        assert_eq!(cqes[0].result(), 0);
        assert_eq!(cqes[0].flags(), 0);

        assert_eq!(
            io_uring_socket
                .local_addr()
                .expect("no local addr")
                .as_socket_ipv4()
                .expect("no IPv4 address")
                .ip(),
            server_addr.as_socket_ipv4().unwrap().ip()
        );
    }

    // Try to listen.
    {
        let op =
            io_uring::opcode::Listen::new(io_uring::types::Fd(io_uring_socket.as_raw_fd()), 128)
                .build()
                .user_data(3456);
        unsafe {
            ring.submission().push(&op.into()).expect("queue is full");
        }
        ring.submit_and_wait(1)?;
        let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
        assert_eq!(cqes.len(), 1);
        assert_eq!(cqes[0].user_data(), 3456);
        assert_eq!(cqes[0].result(), 0);
        assert_eq!(cqes[0].flags(), 0);

        // Ensure the socket is actually in the listening state.
        _ = TcpStream::connect(
            io_uring_socket
                .local_addr()
                .unwrap()
                .as_socket_ipv4()
                .unwrap(),
        )?;
    }

    // Close both sockets, to avoid leaking FDs.
    drop(plain_socket);
    drop(io_uring_socket);

    // Cleanup all fixed files (if any), then reserve slot 0.
    let _ = ring.submitter().unregister_files();
    ring.submitter().register_files_sparse(1).unwrap();

    let fixed_socket_op = opcode::Socket::new(
        Domain::IPV4.into(),
        Type::DGRAM.into(),
        Protocol::UDP.into(),
    );
    let dest_slot = types::DestinationSlot::try_from_slot_target(0).unwrap();
    unsafe {
        ring.submission()
            .push(
                &fixed_socket_op
                    .file_index(Some(dest_slot))
                    .build()
                    .user_data(55)
                    .into(),
            )
            .expect("queue is full");
    }
    ring.submit_and_wait(1)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), 55);
    assert_eq!(cqes[0].result(), 0);
    assert_eq!(cqes[0].flags(), 0);

    // If the fixed-socket operation worked properly, this must not fail.
    ring.submitter().unregister_files().unwrap();

    Ok(())
}

pub fn test_udp_recvmsg_multishot<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    // Multishot recvmsg was introduced in 6.0, like `SendZc`.
    // We cannot probe for the former, so we check for the latter as a proxy instead.
    require!(
        test;
        test.probe.is_supported(opcode::RecvMsgMulti::CODE);
        test.probe.is_supported(opcode::ProvideBuffers::CODE);
        test.probe.is_supported(opcode::SendMsgZc::CODE);
        test.probe.is_supported(opcode::SendMsg::CODE);
    );

    println!("test udp_recvmsg_multishot");

    let server_socket: socket2::Socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().into();
    let server_addr = server_socket.local_addr()?;

    // Provide 2 buffers in buffer group `33`, at index 0 and 1.
    // Each one is 512 bytes large.
    const BUF_GROUP: u16 = 33;
    const SIZE: usize = 512;
    let mut buffers = [[0u8; SIZE]; 3];
    for (index, buf) in buffers.iter_mut().enumerate() {
        let provide_bufs_e = io_uring::opcode::ProvideBuffers::new(
            buf.as_mut_ptr(),
            SIZE as i32,
            1,
            BUF_GROUP,
            index as u16,
        )
        .build()
        .user_data(11)
        .into();
        unsafe { ring.submission().push(&provide_bufs_e)? };
        ring.submitter().submit_and_wait(1)?;
        let cqes: Vec<io_uring::cqueue::Entry> = ring.completion().map(Into::into).collect();
        assert_eq!(cqes.len(), 1);
        assert_eq!(cqes[0].user_data(), 11);
        assert_eq!(cqes[0].result(), 0);
        assert_eq!(cqes[0].flags(), 0);
    }

    // This structure is actually only used for input arguments to the kernel
    // (and only name length and control length are actually relevant).
    let mut msghdr: libc::msghdr = unsafe { std::mem::zeroed() };
    msghdr.msg_namelen = 32;
    msghdr.msg_controllen = 0;

    let recvmsg_e = opcode::RecvMsgMulti::new(
        Fd(server_socket.as_raw_fd()),
        &msghdr as *const _,
        BUF_GROUP,
    )
    .build()
    .user_data(77)
    .into();
    unsafe { ring.submission().push(&recvmsg_e)? };
    ring.submitter().submit().unwrap();

    let client_socket: socket2::Socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().into();
    let client_addr = client_socket
        .local_addr()
        .unwrap()
        .as_socket_ipv4()
        .unwrap();

    let bufs1 = [io::IoSlice::new(b"testfooo for me")];
    let bufs2 = [io::IoSlice::new(b"testbarbar for you and me")];

    let mut msghdr1: libc::msghdr = unsafe { mem::zeroed() };
    msghdr1.msg_name = server_addr.as_ptr() as *const _ as *mut _;
    msghdr1.msg_namelen = server_addr.len();
    msghdr1.msg_iov = bufs1.as_ptr() as *const _ as *mut _;
    msghdr1.msg_iovlen = 1;

    // Non ZeroCopy
    let send_msg_1 = opcode::SendMsg::new(Fd(client_socket.as_raw_fd()), &msghdr1 as *const _)
        .build()
        .user_data(55)
        .into();
    unsafe { ring.submission().push(&send_msg_1)? };
    ring.submitter().submit().unwrap();

    let mut msghdr2: libc::msghdr = unsafe { mem::zeroed() };
    msghdr2.msg_name = server_addr.as_ptr() as *const _ as *mut _;
    msghdr2.msg_namelen = server_addr.len();
    msghdr2.msg_iov = bufs2.as_ptr() as *const _ as *mut _;
    msghdr2.msg_iovlen = 1;

    // ZeroCopy
    let send_msg_2 = opcode::SendMsgZc::new(Fd(client_socket.as_raw_fd()), &msghdr2 as *const _)
        .build()
        .user_data(66)
        .into();
    unsafe { ring.submission().push(&send_msg_2)? };
    ring.submitter().submit().unwrap();

    // Check the completion events for the two UDP messages, plus a trailing
    // CQE signaling that we ran out of buffers.
    ring.submitter().submit_and_wait(5).unwrap();
    let cqes: Vec<io_uring::cqueue::Entry> = ring.completion().map(Into::into).collect();
    assert_eq!(cqes.len(), 5);
    for cqe in cqes {
        let is_more = io_uring::cqueue::more(cqe.flags());
        match cqe.user_data() {
            // send notifications
            55 => {
                assert!(cqe.result() > 0);
                assert!(!is_more);
            }
            // SendMsgZc with two notification
            66 => {
                if cqe.result() > 0 {
                    assert!(is_more);
                } else {
                    assert!(!is_more);
                }
            }
            // RecvMsgMulti
            77 => {
                assert!(cqe.result() > 0);
                assert!(is_more);
                let buf_id = io_uring::cqueue::buffer_select(cqe.flags()).unwrap();
                let tmp_buf = &buffers[buf_id as usize];
                let msg = types::RecvMsgOut::parse(tmp_buf, &msghdr).unwrap();
                assert!([25, 15].contains(&msg.payload_data().len()));
                assert!(!msg.is_payload_truncated());
                assert!(!msg.is_control_data_truncated());
                assert_eq!(msg.control_data(), &[]);
                assert!(!msg.is_name_data_truncated());
                let addr = unsafe {
                    let storage = msg
                        .name_data()
                        .as_ptr()
                        .cast::<libc::sockaddr_storage>()
                        .read_unaligned();
                    let len = msg.name_data().len().try_into().unwrap();
                    socket2::SockAddr::new(storage, len)
                };
                let addr = addr.as_socket_ipv4().unwrap();
                assert_eq!(addr.ip(), client_addr.ip());
                assert_eq!(addr.port(), client_addr.port());
            }
            _ => {
                unreachable!()
            }
        }
    }

    Ok(())
}
pub fn test_udp_recvmsg_multishot_trunc<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::RecvMsgMulti::CODE);
        test.probe.is_supported(opcode::ProvideBuffers::CODE);
        test.probe.is_supported(opcode::SendMsg::CODE);
    );

    println!("test udp_recvmsg_multishot_trunc");

    let server_socket: socket2::Socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().into();
    let server_addr = server_socket.local_addr()?;

    const BUF_GROUP: u16 = 33;
    const DATA: &[u8] = b"testfooo for me";
    let mut buf1 = [0u8; 20]; // 20 = size_of::<io_uring_recvmsg_out>() + msghdr.msg_namelen
    let mut buf2 = [0u8; 20 + DATA.len()];
    let mut buffers = [buf1.as_mut_slice(), buf2.as_mut_slice()];

    for (index, buf) in buffers.iter_mut().enumerate() {
        let provide_bufs_e = opcode::ProvideBuffers::new(
            (**buf).as_mut_ptr(),
            buf.len() as i32,
            1,
            BUF_GROUP,
            index as u16,
        )
        .build()
        .user_data(11)
        .into();
        unsafe { ring.submission().push(&provide_bufs_e)? };
        ring.submitter().submit_and_wait(1)?;
        let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
        assert_eq!(cqes.len(), 1);
        assert_eq!(cqes[0].user_data(), 11);
        assert_eq!(cqes[0].result(), 0);
        assert_eq!(cqes[0].flags(), 0);
    }

    // This structure is actually only used for input arguments to the kernel
    // (and only name length and control length are actually relevant).
    let mut msghdr: libc::msghdr = unsafe { mem::zeroed() };
    msghdr.msg_namelen = 4;

    let recvmsg_e = opcode::RecvMsgMulti::new(
        Fd(server_socket.as_raw_fd()),
        &msghdr as *const _,
        BUF_GROUP,
    )
    .flags(libc::MSG_TRUNC as u32)
    .build()
    .user_data(77)
    .into();
    unsafe { ring.submission().push(&recvmsg_e)? };
    ring.submitter().submit().unwrap();

    let client_socket: socket2::Socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().into();

    let data = [io::IoSlice::new(DATA)];
    let mut msghdr1: libc::msghdr = unsafe { mem::zeroed() };
    msghdr1.msg_name = server_addr.as_ptr() as *const _ as *mut _;
    msghdr1.msg_namelen = server_addr.len();
    msghdr1.msg_iov = data.as_ptr() as *const _ as *mut _;
    msghdr1.msg_iovlen = 1;

    let send_msgs = (0..2)
        .map(|_| {
            opcode::SendMsg::new(Fd(client_socket.as_raw_fd()), &msghdr1 as *const _)
                .build()
                .user_data(55)
                .into()
        })
        .collect::<Vec<_>>();
    unsafe { ring.submission().push_multiple(&send_msgs)? };
    ring.submitter().submit().unwrap();

    ring.submitter().submit_and_wait(4).unwrap();
    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
    assert!([4, 5].contains(&cqes.len()));

    let mut processed_responses = 0;
    for cqe in cqes {
        let is_more = cqueue::more(cqe.flags());
        match cqe.user_data() {
            // send notifications
            55 => {
                assert!(cqe.result() > 0);
                assert!(!is_more);
            }
            // RecvMsgMulti
            77 => {
                if cqe.result() == -105 {
                    // Ran out of buffers
                    continue;
                }

                assert!(cqe.result() > 0);
                assert!(is_more);
                let buf_id = cqueue::buffer_select(cqe.flags()).unwrap() % buffers.len() as u16;
                let tmp_buf = &buffers[buf_id as usize];
                let msg = types::RecvMsgOut::parse(tmp_buf, &msghdr);

                match buf_id {
                    0 => {
                        let msg = msg.unwrap();
                        assert!(msg.is_payload_truncated());
                        assert!(msg.is_name_data_truncated());
                        assert_eq!(DATA.len(), msg.incoming_payload_len() as usize);
                        assert!(msg.payload_data().is_empty());
                        assert!(4 < msg.incoming_name_len());
                        assert_eq!(4, msg.name_data().len());
                    }
                    1 => {
                        let msg = msg.unwrap();
                        assert!(!msg.is_payload_truncated());
                        assert!(msg.is_name_data_truncated());
                        assert_eq!(DATA.len(), msg.incoming_payload_len() as usize);
                        assert_eq!(DATA, msg.payload_data());
                        assert!(4 < msg.incoming_name_len());
                        assert_eq!(4, msg.name_data().len());
                    }
                    _ => unreachable!(),
                }
                processed_responses += 1;
            }
            _ => {
                unreachable!()
            }
        }
    }
    assert_eq!(processed_responses, 2);

    Ok(())
}

pub fn test_udp_send_with_dest<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::Recv::CODE);
        test.probe.is_supported(opcode::Send::CODE);
    );

    println!("test udp_send_with_dest");

    let socket: socket2::Socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().into();
    let addr = socket.local_addr()?;
    let fd = Fd(socket.as_raw_fd());

    // We are going to do a send below. To confirm that it works, start a
    // recv to receive the data sent by the send.
    let mut in_buf = vec![0; 1024];
    let recv = opcode::Recv::new(fd, in_buf.as_mut_ptr(), in_buf.len() as u32)
        .build()
        .user_data(1)
        .into();

    let out_buf = b"test message";

    // For the first send, we don't specify a destination address. This should
    // result in an error.
    let send1 = opcode::Send::new(fd, out_buf.as_ptr(), out_buf.len() as u32)
        .build()
        .user_data(2)
        .into();

    // For the second send, we do specify the destination address, and we should
    // receive our test message there.
    let send2 = opcode::Send::new(fd, out_buf.as_ptr(), out_buf.len() as u32)
        .dest_addr(addr.as_ptr())
        .dest_addr_len(addr.len())
        .build()
        .user_data(3)
        .into();

    unsafe { ring.submission().push_multiple(&[recv, send1, send2])? };
    ring.submitter().submit_and_wait(3)?;

    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
    assert_eq!(cqes.len(), 3);

    for cqe in cqes {
        match cqe.user_data() {
            1 => {
                // The receive, we should have received the test message here.
                let n_received = cqe.result();
                assert_eq!(n_received, out_buf.len() as i32);
                assert_eq!(&in_buf[..n_received as usize], out_buf);
            }
            2 => {
                // The send should have failed because it had no destination address.
                assert_eq!(cqe.result(), -libc::EDESTADDRREQ);
            }
            3 => {
                // The send that should have succeeded.
                let n_sent = cqe.result();
                assert_eq!(n_sent, out_buf.len() as i32);
            }
            _ => unreachable!("We only submit user data 1, 2, and 3."),
        }
    }

    Ok(())
}

pub fn test_udp_sendzc_with_dest<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    // Multishot recvmsg was introduced in 6.0, like `SendZc`.
    // We cannot probe for the former, so we check for the latter as a proxy instead.
    require!(
        test;
        test.probe.is_supported(opcode::RecvMsg::CODE);
        test.probe.is_supported(opcode::ProvideBuffers::CODE);
        test.probe.is_supported(opcode::SendZc::CODE);
    );

    println!("test udp_sendzc_with_dest");

    let server_socket: socket2::Socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().into();
    let dest_addr = server_socket.local_addr()?;

    // Provide 2 buffers in buffer group `33`, at index 0 and 1.
    // Each one is 512 bytes large.
    const BUF_GROUP: u16 = 77;
    const SIZE: usize = 512;
    let mut buffers = [[0u8; SIZE]; 2];
    for (index, buf) in buffers.iter_mut().enumerate() {
        let provide_bufs_e = io_uring::opcode::ProvideBuffers::new(
            buf.as_mut_ptr(),
            SIZE as i32,
            1,
            BUF_GROUP,
            index as u16,
        )
        .build()
        .user_data(11)
        .into();
        unsafe { ring.submission().push(&provide_bufs_e)? };
        ring.submitter().submit_and_wait(1)?;
        let cqes: Vec<io_uring::cqueue::Entry> = ring.completion().map(Into::into).collect();
        assert_eq!(cqes.len(), 1);
        assert_eq!(cqes[0].user_data(), 11);
        assert_eq!(cqes[0].result(), 0);
        assert_eq!(cqes[0].flags(), 0);
    }

    let recvmsg_e = opcode::RecvMulti::new(Fd(server_socket.as_raw_fd()), BUF_GROUP)
        .build()
        .user_data(3)
        .into();
    unsafe { ring.submission().push(&recvmsg_e)? };
    ring.submitter().submit()?;

    let client_socket: socket2::Socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap().into();
    let fd = client_socket.as_raw_fd();

    let buf1 = b"lorep ipsum";

    // 2 self events + 1 recv
    let entry1 = opcode::SendZc::new(Fd(fd), buf1.as_ptr(), buf1.len() as _)
        .dest_addr(dest_addr.as_ptr())
        .dest_addr_len(dest_addr.len())
        .build()
        .user_data(33)
        .flags(Flags::IO_LINK)
        .into();

    unsafe {
        ring.submission().push(&entry1)?;
    }

    // Check the completion events for the two UDP messages, plus a trailing
    // CQE signaling that we ran out of buffers.
    ring.submitter().submit_and_wait(3)?;
    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 3);

    for cqe in cqes {
        match cqe.user_data() {
            // data finally arrived to server
            3 => {
                let buf_index_1 = cqueue::buffer_select(cqe.flags()).unwrap();
                assert_eq!(cqe.result(), 11);
                assert_eq!(&buffers[buf_index_1 as usize][..11], buf1);
            }
            33 => match cqe.result() {
                // First SendZc notification
                11 => {
                    assert!(cqueue::more(cqe.flags()));
                    assert!(!cqueue::notif(cqe.flags()));
                }
                // Last SendZc notification
                0 => {
                    assert!(!cqueue::more(cqe.flags()));
                    assert!(cqueue::notif(cqe.flags()));
                }
                _ => panic!("wrong result for notification"),
            },
            _ => panic!("wrong user_data"),
        }
    }

    Ok(())
}
