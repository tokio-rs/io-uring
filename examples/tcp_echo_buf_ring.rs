use std::collections::VecDeque;
use std::net::TcpListener;
use std::os::unix::io::{AsRawFd, RawFd};
use std::{io, pin::Pin, ptr};

use io_uring::{opcode, squeue, types, sys, IoUring, SubmissionQueue, Submitter};
use slab::Slab;

#[derive(Clone, Debug)]
enum Token {
    Accept,
    Poll {
        fd: RawFd,
    },
    Read {
        fd: RawFd,
    },
    Write {
        fd: RawFd,
        buffer_id: usize,
        offset: usize,
        len: usize,
    },
}

pub struct AcceptCount {
    entry: squeue::Entry,
    count: usize,
}

impl AcceptCount {
    fn new(fd: RawFd, token: usize, count: usize) -> AcceptCount {
        AcceptCount {
            entry: opcode::Accept::new(types::Fd(fd), ptr::null_mut(), ptr::null_mut())
                .build()
                .user_data(token as _),
            count,
        }
    }

    pub fn push_to(&mut self, sq: &mut SubmissionQueue<'_>) {
        while self.count > 0 {
            unsafe {
                match sq.push(&self.entry) {
                    Ok(_) => self.count -= 1,
                    Err(_) => break,
                }
            }
        }

        sq.sync();
    }
}

const BUF_RING_ENTRIES: u16 = 64;
const BUF_RING_BUF_SIZE: usize = 2048;

fn main() -> anyhow::Result<()> {
    let mut ring = IoUring::new(256)?;
    let listener = TcpListener::bind(("127.0.0.1", 3456))?;

    let mut backlog = VecDeque::new();
    // let mut bufpool = Vec::with_capacity(64);
    // let mut buf_alloc = Slab::with_capacity(64);
    let mut token_alloc = Slab::with_capacity(64);

    println!("listen {}", listener.local_addr()?);

    let (submitter, mut sq, mut cq) = ring.split();

    let mut buf_ring = unsafe { types::BufRing::new(BUF_RING_ENTRIES, 1).map_err(io::Error::from_raw_os_error)? };

    unsafe {
        buf_ring.init();
    }

    let mut buf_ring_alloc = Slab::with_capacity(BUF_RING_ENTRIES as usize);

    for _ in 0..BUF_RING_ENTRIES {
        buf_ring_alloc.insert(vec![0u8; BUF_RING_BUF_SIZE].into_boxed_slice());
    }

    for (bid, buf) in buf_ring_alloc.iter() {
        let entry = types::BufRingEntry::new(buf.as_ptr() as u64, buf.len() as u32, bid as u16);

        unsafe {
            buf_ring.add(entry, bid as u32);
        }
    }

    unsafe {
        buf_ring.advance(BUF_RING_ENTRIES as u16);
    }

    let mut accept = AcceptCount::new(listener.as_raw_fd(), token_alloc.insert(Token::Accept), 3);

    accept.push_to(&mut sq);

    println!("starting main loop");
    loop {
        match submitter.submit_and_wait(1) {
            Ok(_) => (),
            Err(ref err) if err.raw_os_error() == Some(libc::EBUSY) => (),
            Err(err) => return Err(err.into()),
        }
        cq.sync();

        // clean backlog
        println!("submitting backlog");
        loop {
            if sq.is_full() {
                match submitter.submit() {
                    Ok(_) => (),
                    Err(ref err) if err.raw_os_error() == Some(libc::EBUSY) => break,
                    Err(err) => return Err(err.into()),
                }
            }
            sq.sync();

            match backlog.pop_front() {
                Some(sqe) => unsafe {
                    let _ = sq.push(&sqe);
                },
                None => break,
            }
        }

        accept.push_to(&mut sq);

        for cqe in &mut cq {
            let ret = cqe.result();
            let token_index = cqe.user_data() as usize;
            let cqe_flags = cqe.flags();

            if ret < 0 {
                eprintln!(
                    "token {:?} error: {:?}",
                    token_alloc.get(token_index),
                    io::Error::from_raw_os_error(-ret)
                );
                continue;
            }

            let token = &mut token_alloc[token_index];
            match token.clone() {
                Token::Accept => {
                    println!("accept");

                    accept.count += 1;

                    let fd = ret;

                    let poll_token = token_alloc.insert(Token::Poll { fd });

                    let poll_e = opcode::PollAdd::new(types::Fd(fd), libc::POLLIN as _)
                        .build()
                        .user_data(poll_token as _);

                    unsafe {
                        if sq.push(&poll_e).is_err() {
                            backlog.push_back(poll_e);
                        }
                    }
                }
                Token::Poll { fd } => {
                    println!("poll {fd}");
                    // let (buf_index, buf) = match bufpool.pop() {
                    //     Some(buf_index) => (buf_index, &mut buf_alloc[buf_index]),
                    //     None => {
                    //         let buf = vec![0u8; 2048].into_boxed_slice();
                    //         let buf_entry = buf_alloc.vacant_entry();
                    //         let buf_index = buf_entry.key();
                    //         (buf_index, buf_entry.insert(buf))
                    //     }
                    // };

                    *token = Token::Read { fd };

                    let recv_e = opcode::Recv::new(
                        types::Fd(fd),
                        std::ptr::null_mut(),
                        2048,
                    )
                    .buf_group(buf_ring.bgid())
                    .flags(1 << sys::IOSQE_BUFFER_SELECT_BIT)
                    .build()
                    .user_data(token_index as _);

                    // let read_e = opcode::Recv::new(types::Fd(fd), buf.as_mut_ptr(), buf.len() as _)
                    //     .build()
                    //     .user_data(token_index as _);

                    unsafe {
                        if sq.push(&recv_e).is_err() {
                            backlog.push_back(recv_e);
                        }
                    }
                }
                Token::Read { fd } => {
                    println!("read {fd}");
                    if ret == 0 {
                        // bufpool.push(buf_index);
                        token_alloc.remove(token_index);

                        println!("shutdown");

                        unsafe {
                            libc::close(fd);
                        }
                    } else {
                        if cqe_flags & io_uring::sys::IORING_CQE_F_BUFFER == 0 {
                            println!("buffer not selected!");
                        } else {
                            println!("IORING_CQE_F_BUFFER set in {:#b}", cqe_flags);
                        }

                        let buffer_id = buf_ring.buffer_id_from_cqe_flags(cqe_flags);

                        println!("kernel chose buffer {buffer_id}");

                        println!("data written to buff {buffer_id}");

                        let len = ret as usize;
                        let buf = &buf_ring_alloc[buffer_id as usize];

                        println!("read {len} bytes: {:02X?}", &buf[0..len]);

                        *token = Token::Write {
                            fd,
                            buffer_id: buffer_id as usize,
                            len,
                            offset: 0,
                        };

                        let write_e = opcode::Send::new(types::Fd(fd), buf.as_ptr(), len as _)
                            .build()
                            .user_data(token_index as _);

                        unsafe {
                            if sq.push(&write_e).is_err() {
                                backlog.push_back(write_e);
                            }
                        }
                    }
                }
                Token::Write {
                    fd,
                    buffer_id,
                    offset,
                    len,
                } => {
                    println!("write {fd}");
                    let write_len = ret as usize;

                    let entry = if offset + write_len >= len {
                        println!("finished write");
                        let buf = &buf_ring_alloc[buffer_id];

                        unsafe {
                            buf_ring.add(types::BufRingEntry::new(buf.as_ptr() as u64, BUF_RING_BUF_SIZE as u32, buffer_id as u16), 0);
                            buf_ring.advance(1);
                        }

                        // bufpool.push(buf_index);

                        *token = Token::Poll { fd };

                        opcode::PollAdd::new(types::Fd(fd), libc::POLLIN as _)
                            .build()
                            .user_data(token_index as _)
                    } else {
                        println!("partial write: {write_len} of {len}");
                        let offset = offset + write_len;
                        let len = len - offset;

                        let buf = &buf_ring_alloc[buffer_id][offset..];

                        *token = Token::Write {
                            fd,
                            buffer_id,
                            offset,
                            len,
                        };

                        opcode::Write::new(types::Fd(fd), buf.as_ptr(), len as _)
                            .build()
                            .user_data(token_index as _)
                    };

                    unsafe {
                        if sq.push(&entry).is_err() {
                            backlog.push_back(entry);
                        }
                    }
                }
            }
        }
    }
}
