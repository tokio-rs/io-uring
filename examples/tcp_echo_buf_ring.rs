use std::collections::VecDeque;
use std::net::TcpListener;
use std::os::unix::io::{AsRawFd, RawFd};
use std::{io, ptr};

use io_uring::{opcode, squeue, types, IoUring, SubmissionQueue};
use slab::Slab;

#[derive(Clone, Debug)]
enum Token {
    Accept,
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

const BUF_RING_ENTRIES: u16 = 1024;
const BUF_RING_BUF_SIZE: usize = 4096;

fn main() -> anyhow::Result<()> {
    let mut ring = IoUring::new(256)?;
    let listener = TcpListener::bind(("127.0.0.1", 3456))?;

    let mut backlog = VecDeque::new();
    let mut token_alloc = Slab::with_capacity(64);

    println!("listen {}", listener.local_addr()?);

    let (submitter, mut sq, mut cq) = ring.split();

    let mut buf_ring = types::BufRing::new(BUF_RING_ENTRIES, BUF_RING_BUF_SIZE as u32, 1)
        .map_err(io::Error::from_raw_os_error)?;

    buf_ring.init();

    submitter.register_buf_ring(buf_ring.ring_addr(), buf_ring.entries(), buf_ring.bgid())?;

    println!("registered buffer ring");

    for idx in 0..BUF_RING_ENTRIES {
        unsafe {
            buf_ring.add(idx, idx);
        }
    }

    unsafe {
        buf_ring.advance(BUF_RING_ENTRIES);
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
                    accept.count += 1;

                    let fd = ret;

                    let read_token = token_alloc.insert(Token::Read { fd });

                    let recv_e = opcode::RecvMulti::new(types::Fd(fd), buf_ring.bgid())
                        .build()
                        .user_data(read_token as _);

                    unsafe {
                        if sq.push(&recv_e).is_err() {
                            backlog.push_back(recv_e);
                        }
                    }
                }
                Token::Read { fd } => {
                    if ret == 0 {
                        token_alloc.remove(token_index);

                        println!("shutdown");

                        unsafe {
                            libc::close(fd);
                        }
                    } else {
                        if let Some(buffer_id) = buf_ring.buffer_id_from_cqe_flags(cqe_flags) {
                            let len = ret as usize;
                            let buf = unsafe { buf_ring.read_buffer(buffer_id) };

                            let write_token = token_alloc.insert(Token::Write {
                                fd,
                                buffer_id: buffer_id as usize,
                                len,
                                offset: 0,
                            });

                            let write_e = opcode::Send::new(types::Fd(fd), buf.as_ptr(), len as _)
                                .build()
                                .user_data(write_token as _);

                            unsafe {
                                if sq.push(&write_e).is_err() {
                                    backlog.push_back(write_e);
                                }
                            }
                        } else {
                            eprintln!("token {:?} error: buffer not selected", token);
                        }
                    }
                }
                Token::Write {
                    fd,
                    buffer_id,
                    offset,
                    len,
                } => {
                    let write_len = ret as usize;

                    if offset + write_len >= len {
                        unsafe {
                            buf_ring.add(buffer_id as u16, 0);
                            buf_ring.advance(1);
                        }
                    } else {
                        let offset = offset + write_len;
                        let len = len - offset;

                        let buf = unsafe { &buf_ring.read_buffer(buffer_id as u16)[offset..len] };

                        *token = Token::Write {
                            fd,
                            buffer_id,
                            offset,
                            len,
                        };

                        let entry = opcode::Write::new(types::Fd(fd), buf.as_ptr(), len as _)
                            .build()
                            .user_data(token_index as _);

                        unsafe {
                            if sq.push(&entry).is_err() {
                                backlog.push_back(entry);
                            }
                        }
                    };
                }
            }
        }
    }
}
