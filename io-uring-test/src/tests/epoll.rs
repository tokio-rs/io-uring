use crate::Test;
use ::core::{mem::MaybeUninit, time::Duration};
use ::std::{
    io::{PipeReader, PipeWriter, Read, Write},
    os::fd::{AsFd, FromRawFd, RawFd},
    thread,
};
use io_uring::{cqueue, opcode, squeue, types, IoUring};
use std::os::unix::io::AsRawFd;

// Tests translated from liburing/test/epwait.c.

const REQ_TYPE_EPOLL_WAIT: u64 = 1;
const DATA: &[u8] = b"foo";

#[derive(Debug)]
struct RxTxPipe {
    rx: PipeReader,
    tx: PipeWriter,
}

pub fn test_ready<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::EpollWait::CODE);
    );

    println!("test epoll_wait_ready");

    const NPIPES: usize = 2;

    let (epfd, mut pipes, mut events) = init::<NPIPES>()?;

    // write

    for pipe in &mut pipes {
        pipe.tx.write_all(DATA)?;
    }

    // submit epoll_wait

    let sqe = opcode::EpollWait::new(
        types::Fd(epfd.as_raw_fd()),
        events.as_mut_ptr().cast(),
        NPIPES as _,
    )
    .build()
    .user_data(REQ_TYPE_EPOLL_WAIT)
    .into();
    unsafe { ring.submission().push(&sqe) }?;

    // process completions

    ring.submit_and_wait(1)?;
    let cqe = ring
        .completion()
        .map(Into::<cqueue::Entry>::into)
        .next()
        .unwrap();
    assert_eq!(cqe.user_data(), REQ_TYPE_EPOLL_WAIT);
    assert_eq!(cqe.result(), 2);

    // read

    for rx_idx in events {
        let mut tmp = [0u8; 16];
        let len = pipes[rx_idx.u64 as usize].rx.read(&mut tmp)?;
        assert_eq!(len, DATA.len());
        assert_eq!(&tmp[..len], DATA);
    }

    Ok(())
}

pub fn test_not_ready<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::EpollWait::CODE);
    );

    println!("test epoll_wait_not_ready");

    const NPIPES: usize = 2;
    let (epfd, mut pipes, mut events) = init::<NPIPES>()?;

    // submit epoll_wait

    let sqe = opcode::EpollWait::new(
        types::Fd(epfd.as_raw_fd()),
        events.as_mut_ptr().cast(),
        NPIPES as _,
    )
    .build()
    .user_data(REQ_TYPE_EPOLL_WAIT)
    .into();
    unsafe { ring.submission().push(&sqe) }?;

    // write

    for pipe in &mut pipes {
        thread::sleep(Duration::from_micros(10000));
        pipe.tx.write_all(DATA)?;
    }

    // process completions

    let mut nr = 0;
    ring.submit_and_wait(1)?;
    for cqe in ring.completion().map(Into::<cqueue::Entry>::into).take(1) {
        assert_eq!(cqe.user_data(), REQ_TYPE_EPOLL_WAIT);
        assert!(0 <= cqe.result() && cqe.result() <= 2);
        nr = cqe.result();
    }

    // read

    for rx_idx in events.iter().take(nr as _) {
        let mut tmp = [0u8; 16];
        let len = pipes[rx_idx.u64 as usize].rx.read(&mut tmp)?;
        assert_eq!(len, DATA.len());
        assert_eq!(&tmp[..len], DATA);
    }

    Ok(())
}

pub fn test_delete<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::EpollWait::CODE);
    );

    println!("test epoll_wait_delete");

    const NPIPES: usize = 2;
    let (epfd, mut pipes, mut events) = init::<NPIPES>()?;

    // submit epoll_wait

    let sqe = opcode::EpollWait::new(
        types::Fd(epfd.as_raw_fd()),
        events.as_mut_ptr().cast(),
        NPIPES as _,
    )
    .build()
    .user_data(REQ_TYPE_EPOLL_WAIT)
    .into();
    unsafe { ring.submission().push(&sqe) }?;

    // delete event 0

    {
        let mut event = ::libc::epoll_event {
            events: ::libc::EPOLLIN.cast_unsigned(),
            u64: 0,
        };
        let res = unsafe {
            ::libc::epoll_ctl(
                epfd,
                ::libc::EPOLL_CTL_DEL,
                pipes[0].rx.as_raw_fd(),
                &mut event,
            )
        };
        assert!(res >= 0);
    }

    // write

    for pipe in &mut pipes {
        pipe.tx.write_all(DATA)?;
    }

    // process completions

    ring.submit_and_wait(1)?;
    for cqe in ring.completion().map(Into::<cqueue::Entry>::into).take(1) {
        assert_eq!(cqe.user_data(), REQ_TYPE_EPOLL_WAIT);
        // check for only one event
        assert!(cqe.result() == 1);
    }

    // check that both writes still happened

    for pipe in &mut pipes {
        let mut tmp = [0u8; 16];
        let len = pipe.rx.read(&mut tmp)?;
        assert_eq!(len, DATA.len());
        assert_eq!(&tmp[..len], DATA);
    }

    // re-register event 0

    {
        let mut event = ::libc::epoll_event {
            events: ::libc::EPOLLIN.cast_unsigned(),
            u64: 0,
        };
        unsafe {
            ::libc::epoll_ctl(
                epfd,
                ::libc::EPOLL_CTL_ADD,
                pipes[0].rx.as_raw_fd(),
                &mut event,
            )
        };
    }

    Ok(())
}

pub fn test_remove<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::EpollWait::CODE);
    );

    println!("test epoll_wait_remove");

    const NPIPES: usize = 2;
    let (epfd, mut pipes, mut events) = init::<NPIPES>()?;

    // submit epoll_wait

    let sqe = opcode::EpollWait::new(
        types::Fd(epfd.as_raw_fd()),
        events.as_mut_ptr().cast(),
        NPIPES as _,
    )
    .build()
    .user_data(REQ_TYPE_EPOLL_WAIT)
    .into();
    unsafe { ring.submission().push(&sqe) }?;

    // force close `epfd` before processing

    let epfd = unsafe { ::std::os::fd::OwnedFd::from_raw_fd(epfd) };
    drop(epfd);

    // write

    thread::sleep(Duration::from_micros(10000));
    for pipe in &mut pipes {
        pipe.tx.write_all(DATA)?;
    }

    // process completions

    ring.submit_and_wait(1)?;
    for cqe in ring.completion().map(Into::<cqueue::Entry>::into).take(1) {
        assert_eq!(cqe.user_data(), REQ_TYPE_EPOLL_WAIT);
        let err = cqe.result();
        // check that we got the expected errors
        assert!([-::libc::EAGAIN, -::libc::EBADF].contains(&err));
    }

    Ok(())
}

pub fn test_race<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::EpollWait::CODE);
    );

    println!("test epoll_wait_race");

    const LOOPS: usize = 500;
    const NPIPES: usize = 8;

    fn prune_read(
        readers: &mut [PipeReader],
        events: &[::libc::epoll_event],
        nr: usize,
    ) -> anyhow::Result<()> {
        for rx_idx in events.iter().take(nr) {
            let mut tmp = [0u8; 16];
            // read
            let len = readers[rx_idx.u64 as usize].read(&mut tmp)?;
            // account for possibility of multiple writes
            assert!(len >= DATA.len());
            // verify each chunk matches write data
            for chunk in tmp[..len].chunks(DATA.len()) {
                assert_eq!(chunk, DATA);
            }
        }
        Ok(())
    }

    thread::scope(|scope| -> anyhow::Result<()> {
        let (efd, pipes, mut events) = init::<NPIPES>()?;

        let mut readers = vec![];
        let mut writers = vec![];
        for pipe in pipes {
            readers.push(pipe.rx);
            writers.push(pipe.tx);
        }

        // spawn writers in a thread
        let handle = scope.spawn(move || -> anyhow::Result<()> {
            for _ in 0..LOOPS {
                thread::sleep(Duration::from_micros(150));
                for tx in &mut writers {
                    tx.write_all(DATA)?;
                }
            }
            Ok(())
        });

        // repeatedly submit epoll_wait and process completions
        for _ in 0..LOOPS {
            let sqe = opcode::EpollWait::new(
                types::Fd(efd.as_raw_fd()),
                events.as_mut_ptr().cast(),
                NPIPES as _,
            )
            .build()
            .user_data(REQ_TYPE_EPOLL_WAIT)
            .into();
            unsafe { ring.submission().push(&sqe) }?;

            ring.submit_and_wait(1)?;
            let cqe = ring
                .completion()
                .next()
                .map(Into::<cqueue::Entry>::into)
                .unwrap();
            assert_eq!(cqe.user_data(), REQ_TYPE_EPOLL_WAIT);
            assert!(cqe.result() >= 0);
            let nr = cqe.result();

            // process the events
            prune_read(&mut readers, &events, nr as _)?;
            thread::sleep(Duration::from_micros(100));
        }

        handle.join().unwrap()?;

        Ok(())
    })?;

    Ok(())
}

fn init<const NPIPES: usize>(
) -> anyhow::Result<(RawFd, [RxTxPipe; NPIPES], [::libc::epoll_event; NPIPES])> {
    let pipes: [RxTxPipe; NPIPES] = {
        let mut pipes: [MaybeUninit<RxTxPipe>; NPIPES] = [const { MaybeUninit::uninit() }; NPIPES];
        for pipe in &mut pipes {
            let (rx, tx) = std::io::pipe()?;
            pipe.write(RxTxPipe { rx, tx });
        }
        unsafe { ::core::mem::transmute_copy(&pipes) }
    };

    let epfd = unsafe { ::libc::epoll_create1(0) };
    assert!(epfd >= 0);

    for (pipe, rx_idx) in pipes.iter().zip(0u64..) {
        let rx = pipe.rx.as_fd();
        let mut events = ::libc::epoll_event {
            events: ::libc::EPOLLIN.cast_unsigned(),
            u64: rx_idx,
        };
        let res = unsafe {
            ::libc::epoll_ctl(
                epfd,
                ::libc::EPOLL_CTL_ADD,
                rx.as_raw_fd() as _,
                &mut events,
            )
        };
        assert!(res >= 0);
    }

    let events = unsafe { ::core::mem::zeroed() };

    Ok((epfd, pipes, events))
}
