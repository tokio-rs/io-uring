use std::{ io, ptr };
use std::os::unix::io::{ AsRawFd, RawFd };
use std::net::TcpListener;
use slab::Slab;
use io_uring::opcode::{ self, types };
use io_uring::{ squeue, IoUring };


#[derive(Clone, Debug)]
enum Token {
    Accept,
    Poll { fd: RawFd },
    Read {
        fd: RawFd,
        buf_index: usize,
        bufv_index: usize
    },
    Write {
        fd: RawFd,
        buf_index: usize,
        bufv_index: usize,
        offset: usize,
        len: usize
    }
}

pub struct AcceptCount {
    entry: squeue::Entry,
    count: usize
}

impl AcceptCount {
    fn new(fd: RawFd, token: usize, count: usize) -> AcceptCount {
        AcceptCount {
            entry: opcode::Accept::new(
                types::Target::Fd(fd),
                ptr::null_mut(), ptr::null_mut()
            )
                .build()
                .user_data(token as _),
            count
        }
    }

    pub fn push(&mut self, sq: &mut squeue::AvailableQueue) {
        while self.count > 0 {
            unsafe {
                match sq.push(self.entry.clone()) {
                    Ok(_) => self.count -= 1,
                    Err(_) => break
                }
            }
        }
    }
}


fn main() -> anyhow::Result<()> {
    let mut ring = IoUring::new(256)?;
    let listener = TcpListener::bind(("127.0.0.1", 3456))?;

    let mut backlog = Vec::new();
    let mut bufpool = Vec::with_capacity(64);
    let mut buf_alloc = Slab::with_capacity(64);
    let mut bufv_alloc = Slab::with_capacity(64);
    let mut token_alloc = Slab::with_capacity(64);

    println!("listen {}", listener.local_addr()?);

    let (submitter, sq, cq) = ring.split();

    let mut accept = AcceptCount::new(
        listener.as_raw_fd(),
        token_alloc.insert(Token::Accept),
        3
    );

    accept.push(&mut sq.available());

    loop {
        submitter.submit_and_wait(1)?;

        let mut sq = sq.available();
        let mut iter =  backlog.drain(..);

        // clean backlog
        loop {
            if sq.is_full() {
                match submitter.submit() {
                    Ok(_) => (),
                    Err(ref err) if err.raw_os_error() == Some(libc::EBUSY) => break,
                    Err(err) => return Err(err.into())
                }
                sq.sync();
            }

            match iter.next() {
                Some(sqe) => unsafe {
                    let _ = sq.push(sqe);
                },
                None => break
            }
        }

        drop(iter);

        accept.push(&mut sq);

        for cqe in cq.available() {
            let ret = cqe.result();
            let token_index = cqe.user_data() as usize;

            if ret >= 0 {
                let token = token_alloc.get_mut(token_index)
                    .ok_or_else(|| anyhow::format_err!("not found token"))?;
                let token_clone = token.clone();
                match token_clone {
                    Token::Accept => {
                        println!("accept");

                        accept.count += 1;

                        let fd = ret;
                        let poll_token = token_alloc.insert(Token::Poll { fd });

                        let poll_e = opcode::PollAdd::new(types::Target::Fd(fd), libc::POLLIN)
                            .build()
                            .user_data(poll_token as _);

                        unsafe {
                            if let Err(entry) = sq.push(poll_e) {
                                backlog.push(entry);
                            }
                        }
                    },
                    Token::Poll { fd } => {
                        let (buf_index, _, bufv_index, bufv) = match bufpool.pop() {
                            Some((buf_index, bufv_index)) => {
                                let buf = buf_alloc.get_mut(buf_index)
                                    .ok_or_else(|| anyhow::format_err!("not found buf"))?;
                                let bufv = bufv_alloc.get_mut(bufv_index)
                                    .ok_or_else(|| anyhow::format_err!("not found bufv"))?;
                                (buf_index, buf, bufv_index, bufv)
                            },
                            None => {
                                let (buf_index, buf) = {
                                    let buf = vec![0u8; 2048].into_boxed_slice();
                                    let buf_entry = buf_alloc.vacant_entry();
                                    let buf_index = buf_entry.key();
                                    (buf_index, buf_entry.insert(buf))
                                };

                                let (bufv_index, bufv) = {
                                    let bufv = io::IoSliceMut::new(&mut buf[..]);
                                    let bufv_entry = bufv_alloc.vacant_entry();
                                    let bufv_index = bufv_entry.key();
                                    (bufv_index, bufv_entry.insert(as_iovec_mut(bufv)))
                                };

                                (buf_index, buf, bufv_index, bufv)
                            }
                        };

                        let read_token =
                            token_alloc.insert(Token::Read { fd, buf_index, bufv_index });

                        let read_e =
                            opcode::Readv::new(types::Target::Fd(fd), bufv, 1)
                                .build()
                                .user_data(read_token as _);

                        unsafe {
                            if let Err(entry) = sq.push(read_e) {
                                backlog.push(entry);
                            }
                        }
                    },
                    Token::Read { fd, buf_index, bufv_index } => if ret == 0 {
                        bufpool.push((buf_index, bufv_index));
                        token_alloc.remove(token_index);

                        println!("shutdown");

                        unsafe {
                            libc::close(fd);
                        }
                    } else {
                        let len = ret as usize;
                        let buf = buf_alloc.get(buf_index)
                            .ok_or_else(|| anyhow::format_err!("not found buf"))?;
                        let bufv = bufv_alloc.get_mut(bufv_index)
                            .ok_or_else(|| anyhow::format_err!("not found bufv"))?;

                        *bufv = as_iovec(io::IoSlice::new(&buf[..len]));
                        *token = Token::Write {
                            fd, buf_index, bufv_index, len,
                            offset: 0
                        };

                        let write_e =
                            opcode::Writev::new(types::Target::Fd(fd), bufv, 1)
                                .build()
                                .user_data(token_index as _);

                        unsafe {
                            if let Err(entry) = sq.push(write_e) {
                                backlog.push(entry);
                            }
                        }
                    },
                    Token::Write { fd, buf_index, bufv_index, offset, len } => {
                        let write_len = ret as usize;

                        let entry = if offset + write_len >= len {
                            bufpool.push((buf_index, bufv_index));

                            *token = Token::Poll { fd };

                            opcode::PollAdd::new(types::Target::Fd(fd), libc::POLLIN)
                                .build()
                                .user_data(token_index as _)
                        } else {
                            let buf = buf_alloc.get(buf_index)
                                .ok_or_else(|| anyhow::format_err!("not found buf"))?;
                            let bufv = bufv_alloc.get_mut(bufv_index)
                                .ok_or_else(|| anyhow::format_err!("not found bufv"))?;

                            let offset = offset + write_len;
                            let len = len - offset;

                            *bufv = as_iovec(io::IoSlice::new(&buf[offset..][..len]));
                            *token = Token::Write { fd, buf_index, bufv_index, offset, len };

                            opcode::Writev::new(types::Target::Fd(fd), bufv, 1)
                                .build()
                                .user_data(token_index as _)
                        };

                        unsafe {
                            if let Err(entry) = sq.push(entry) {
                                backlog.push(entry);
                            }
                        }
                    }
                }
            } else {
                eprintln!("token {:?} error: {:?}", token_alloc.get(token_index), io::Error::from_raw_os_error(-ret));
            }
        }
    }
}

fn as_iovec(bufv: io::IoSlice<'_>) -> libc::iovec {
    unsafe {
        std::mem::transmute(bufv)
    }
}

fn as_iovec_mut(bufv: io::IoSliceMut<'_>) -> libc::iovec {
    unsafe {
        std::mem::transmute(bufv)
    }
}
