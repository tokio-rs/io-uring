use criterion::{black_box, criterion_group, criterion_main, Criterion};
use io_uring::{opcode, IoUring};

struct TaskQueue(usize);

impl TaskQueue {
    pub fn want(&self) -> bool {
        self.0 != 0
    }

    pub fn pop(&mut self) {
        self.0 -= 1;
    }
}

fn bench_normal(c: &mut Criterion) {
    let mut io_uring = IoUring::new(16).unwrap();

    c.bench_function("normal", |b| {
        b.iter(|| {
            let mut queue = TaskQueue(128);

            while queue.want() {
                {
                    let mut sq = io_uring.submission();
                    while queue.want() {
                        unsafe {
                            match sq.push(&black_box(opcode::Nop::new()).build()) {
                                Ok(_) => queue.pop(),
                                Err(_) => break,
                            }
                        }
                    }
                }

                io_uring.submit_and_wait(16).unwrap();

                io_uring.completion().map(black_box).for_each(drop);
            }
        });
    });
}

fn bench_prepare(c: &mut Criterion) {
    let mut io_uring = IoUring::new(16).unwrap();

    c.bench_function("prepare", |b| {
        b.iter(|| {
            let mut queue = TaskQueue(128);

            while queue.want() {
                {
                    let mut sq = io_uring.submission();
                    while queue.want() {
                        unsafe {
                            match sq.push_command(&black_box(opcode::Nop::new()), None) {
                                Ok(_) => queue.pop(),
                                Err(_) => break,
                            }
                        }
                    }
                }

                io_uring.submit_and_wait(16).unwrap();

                io_uring.completion().map(black_box).for_each(drop);
            }
        });
    });
}

fn bench_prepare_sqe(c: &mut Criterion) {
    let mut io_uring = IoUring::new(16).unwrap();

    c.bench_function("prepare_sqe", |b| {
        b.iter(|| {
            let mut queue = TaskQueue(128);

            while queue.want() {
                {
                    let mut sq = io_uring.submission();
                    while queue.want() {
                        unsafe {
                            match sq.get_available_sqe(0) {
                                Ok(sqe) => {
                                    let nop_sqe: &mut opcode::NopSqe = black_box(sqe.into());
                                    nop_sqe.prepare();
                                    sq.move_forward(1);
                                    queue.pop();
                                }
                                Err(_) => break,
                            };
                        }
                    }
                }

                io_uring.submit_and_wait(16).unwrap();

                io_uring.completion().map(black_box).for_each(drop);
            }
        });
    });
}

criterion_group!(squeue, bench_normal, bench_prepare, bench_prepare_sqe);
criterion_main!(squeue);
