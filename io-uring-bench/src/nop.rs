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
                    let mut sq = io_uring.submission().available();
                    while queue.want() {
                        unsafe {
                            match sq.push(black_box(opcode::Nop::new()).build()) {
                                Ok(_) => queue.pop(),
                                Err(_) => break,
                            }
                        }
                    }
                }

                io_uring.submit_and_wait(16).unwrap();

                io_uring
                    .completion()
                    .available()
                    .map(black_box)
                    .for_each(drop);
            }
        });
    });
}

#[cfg(not(feature = "concurrent"))]
fn bench_concurrent(_: &mut Criterion) {}

#[cfg(feature = "concurrent")]
fn bench_concurrent(c: &mut Criterion) {
    use crossbeam_utils::thread;
    use std::sync::{atomic, Barrier};
    use std::time::{Duration, Instant};

    const RING_SIZE: usize = 128;

    let io_uring = IoUring::new(RING_SIZE as u32).unwrap();
    let io_uring = io_uring.concurrent();

    c.bench_function("concurrent push uncontended", |b| {
        b.iter_custom(|iters| {
            let mut time = Duration::default();

            for iteration_num in (0..iters).step_by(RING_SIZE) {
                let amt = std::cmp::min(RING_SIZE as u64, iters - iteration_num);

                let sq = io_uring.submission();
                let start = Instant::now();
                for _ in 0..amt {
                    let _ = unsafe { sq.push(black_box(opcode::Nop::new().build())) };
                }
                time += start.elapsed();

                io_uring.submit_and_wait(amt as usize).unwrap();
                while let Some(cqe) = io_uring.completion().pop() {
                    black_box(cqe);
                }
            }

            time
        });
    });

    c.bench_function("concurrent pop uncontended", |b| {
        b.iter_custom(|iters| {
            let mut time = Duration::default();

            for iteration_num in (0..iters).step_by(RING_SIZE) {
                let amt = std::cmp::min(RING_SIZE as u64, iters - iteration_num);

                let sq = io_uring.submission();
                for _ in 0..amt {
                    let _ = unsafe { sq.push(black_box(opcode::Nop::new().build())) };
                }
                io_uring.submit_and_wait(amt as usize).unwrap();

                let start = Instant::now();
                for _ in 0..amt {
                    black_box(io_uring.completion().pop().unwrap());
                }
                time += start.elapsed();
            }

            time
        });
    });

    c.bench_function("concurrent push contended", |b| {
        const CONTENTION: usize = 4;

        let barrier = Barrier::new(CONTENTION);
        let stop = atomic::AtomicBool::new(false);

        thread::scope(|s| {
            for _ in 0..CONTENTION - 1 {
                s.spawn(|_| {
                    loop {
                        // Wait for the benchmarking thread to be ready to benchmark.
                        barrier.wait();

                        if stop.load(atomic::Ordering::SeqCst) {
                            break;
                        }

                        let sq = io_uring.submission();
                        for _ in 0..RING_SIZE / CONTENTION {
                            let _ = unsafe { sq.push(black_box(opcode::Nop::new().build())) };
                            //std::thread::yield_now();
                        }

                        // Wait for the other threads to finish pushing.
                        barrier.wait();
                    }
                });
            }

            b.iter_custom(|iters| {
                let mut time = Duration::default();

                for iteration_num in (0..iters).step_by(RING_SIZE / CONTENTION) {
                    let amt = std::cmp::min((RING_SIZE / CONTENTION) as u64, iters - iteration_num);

                    // Wait for other threads to be ready to contend.
                    barrier.wait();

                    let sq = io_uring.submission();
                    let start = Instant::now();
                    for _ in 0..amt {
                        let _ = unsafe { sq.push(black_box(opcode::Nop::new().build())) };
                    }
                    time += start.elapsed();

                    // Wait for the other threads to finish pushing.
                    barrier.wait();

                    let pushed = RING_SIZE - (RING_SIZE / CONTENTION) + amt as usize;
                    io_uring.submit_and_wait(pushed).unwrap();
                    while let Some(cqe) = io_uring.completion().pop() {
                        black_box(cqe);
                    }
                }

                time
            });

            stop.store(true, atomic::Ordering::SeqCst);
            barrier.wait();
        })
        .unwrap();
    });

    c.bench_function("concurrent pop contended", |b| {
        const CONTENTION: usize = 4;

        let barrier = Barrier::new(CONTENTION);
        let stop = atomic::AtomicBool::new(false);

        thread::scope(|s| {
            for _ in 0..CONTENTION - 1 {
                s.spawn(|_| {
                    loop {
                        // Wait for the benchmarking thread to be ready to benchmark.
                        barrier.wait();

                        if stop.load(atomic::Ordering::SeqCst) {
                            break;
                        }

                        let cq = io_uring.completion();
                        for _ in 0..RING_SIZE / CONTENTION {
                            black_box(cq.pop().unwrap());
                        }

                        // Wait for the other threads to finish popping.
                        barrier.wait();
                    }
                });
            }

            b.iter_custom(|iters| {
                let mut time = Duration::default();

                for iteration_num in (0..iters).step_by(RING_SIZE / CONTENTION) {
                    let amt = std::cmp::min((RING_SIZE / CONTENTION) as u64, iters - iteration_num);

                    let popped = RING_SIZE - (RING_SIZE / CONTENTION) + amt as usize;
                    let sq = io_uring.submission();
                    for _ in 0..popped {
                        assert!(unsafe { sq.push(black_box(opcode::Nop::new().build())) }.is_ok());
                    }
                    io_uring.submit_and_wait(popped).unwrap();

                    // Wait for other threads to be ready to contend.
                    barrier.wait();

                    let cq = io_uring.completion();
                    let start = Instant::now();
                    for _ in 0..amt {
                        black_box(cq.pop().unwrap());
                        std::thread::yield_now();
                    }
                    time += start.elapsed();

                    // Wait for the other threads to finish popping.
                    barrier.wait();
                }

                time
            });

            stop.store(true, atomic::Ordering::SeqCst);
            barrier.wait();
        })
        .unwrap();
    });
}

criterion_group!(squeue, bench_normal, bench_concurrent);
criterion_main!(squeue);
