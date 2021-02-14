use criterion::black_box;

const N: usize = 8;
const ITER: usize = 8;

fn bench_io_uring() {
    use io_uring::{opcode, IoUring};

    let mut io_uring = IoUring::new(N as _).unwrap();

    for _ in 0..ITER {
        let mut sq = io_uring.submission().available();

        for i in 0..N {
            let nop_e = opcode::Nop::new().build().user_data(black_box(i as _));
            unsafe {
                sq.push(nop_e).ok().unwrap();
            }
        }

        drop(sq);

        io_uring.submit_and_wait(N).unwrap();

        io_uring
            .completion()
            .available()
            .map(black_box)
            .for_each(drop);
    }
}

#[cfg(feature = "unstable")]
fn bench_io_uring_batch() {
    use io_uring::{opcode, IoUring};
    use std::mem;

    let mut io_uring = IoUring::new(N as _).unwrap();
    let sqes = [
        opcode::Nop::new().build().user_data(black_box(0)),
        opcode::Nop::new().build().user_data(black_box(1)),
        opcode::Nop::new().build().user_data(black_box(2)),
        opcode::Nop::new().build().user_data(black_box(3)),
        opcode::Nop::new().build().user_data(black_box(4)),
        opcode::Nop::new().build().user_data(black_box(5)),
        opcode::Nop::new().build().user_data(black_box(6)),
        opcode::Nop::new().build().user_data(black_box(7)),
    ];
    let mut cqes = [
        mem::MaybeUninit::uninit(),
        mem::MaybeUninit::uninit(),
        mem::MaybeUninit::uninit(),
        mem::MaybeUninit::uninit(),
        mem::MaybeUninit::uninit(),
        mem::MaybeUninit::uninit(),
        mem::MaybeUninit::uninit(),
        mem::MaybeUninit::uninit(),
    ];

    for _ in 0..ITER {
        unsafe {
            io_uring
                .submission()
                .available()
                .push_multiple(&sqes)
                .ok()
                .unwrap();
        }

        io_uring.submit_and_wait(N).unwrap();

        let n = io_uring.completion().available().fill(&mut cqes);

        assert_eq!(n, N);

        cqes.iter().map(black_box).for_each(drop);
    }
}

fn bench_iou() {
    use iou::IoUring;

    let mut io_uring = IoUring::new(N as _).unwrap();

    for _ in 0..ITER {
        let mut sq = io_uring.sq();

        for i in 0..N {
            unsafe {
                let mut sqe = sq.prepare_sqe().unwrap();
                sqe.prep_nop();
                sqe.set_user_data(black_box(i as _));
            }
        }

        sq.submit_and_wait(N as _).unwrap();

        io_uring.cqes().map(black_box).for_each(drop);
    }
}

fn bench_uring_sys() {
    use std::{mem, ptr};
    use uring_sys::*;

    let mut io_uring = mem::MaybeUninit::<io_uring>::uninit();

    unsafe {
        if io_uring_queue_init(N as _, io_uring.as_mut_ptr(), 0) != 0 {
            panic!()
        }
    }

    let mut io_uring = unsafe { io_uring.assume_init() };

    for _ in 0..ITER {
        for i in 0..N {
            unsafe {
                let sqe = io_uring_get_sqe(&mut io_uring);

                if sqe.is_null() {
                    panic!()
                }

                io_uring_prep_nop(sqe);
                io_uring_sqe_set_data(sqe, i as _);
            }
        }

        unsafe {
            if io_uring_submit_and_wait(&mut io_uring, N as _) < 0 {
                panic!()
            }
        }

        loop {
            unsafe {
                let mut cqe = ptr::null_mut();
                io_uring_peek_cqe(&mut io_uring, &mut cqe);

                if cqe.is_null() {
                    break;
                }

                black_box(cqe);

                io_uring_cq_advance(&mut io_uring, 1);
            }
        }
    }
}

fn bench_uring_sys_batch() {
    use std::{mem, ptr};
    use uring_sys::*;

    let mut io_uring = mem::MaybeUninit::<io_uring>::uninit();

    unsafe {
        if io_uring_queue_init(N as _, io_uring.as_mut_ptr(), 0) != 0 {
            panic!()
        }
    }

    let mut io_uring = unsafe { io_uring.assume_init() };
    let mut cqes = [ptr::null_mut(); 8];

    for _ in 0..ITER {
        for i in 0..N {
            unsafe {
                let sqe = io_uring_get_sqe(&mut io_uring);

                if sqe.is_null() {
                    panic!()
                }

                io_uring_prep_nop(sqe);
                io_uring_sqe_set_data(sqe, i as _);
            }
        }

        unsafe {
            if io_uring_submit_and_wait(&mut io_uring, N as _) < 0 {
                panic!()
            }
        }

        unsafe {
            if io_uring_peek_batch_cqe(&mut io_uring, cqes.as_mut_ptr(), cqes.len() as _) != 8 {
                panic!()
            }

            io_uring_cq_advance(&mut io_uring, 8);
        }

        cqes.iter().copied().map(black_box).for_each(drop);
    }
}

#[cfg(feature = "unstable")]
iai::main!(
    bench_io_uring,
    bench_io_uring_batch,
    bench_iou,
    bench_uring_sys,
    bench_uring_sys_batch
);

#[cfg(not(feature = "unstable"))]
iai::main!(
    bench_io_uring,
    bench_iou,
    bench_uring_sys,
    bench_uring_sys_batch
);
