use criterion::{ criterion_main, criterion_group, Criterion, black_box };
use io_uring::{ opcode, IoUring };


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
                                Err(_) => break
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
    let io_uring = IoUring::new(16).unwrap();
    let io_uring = io_uring.concurrent();

    c.bench_function("concurrent", |b| {
        b.iter(|| {
            let mut queue = TaskQueue(128);

            while queue.want() {
                let sq = io_uring.submission();
                while queue.want() {
                    unsafe {
                        match sq.push(black_box(opcode::Nop::new()).build()) {
                            Ok(_) => queue.pop(),
                            Err(_) => break
                        }
                    }
                }

                io_uring.submit_and_wait(16).unwrap();

                let cq = io_uring.completion();
                while let Some(cqe) = cq.pop() {
                    black_box(cqe);
                }
            }
        });
    });
}

fn bench_iou(c: &mut Criterion) {
    use iou::IoUring;

    let mut io_uring = IoUring::new(16).unwrap();

    c.bench_function("iou", |b| {
        b.iter(|| {
            let mut queue = TaskQueue(128);

            while queue.want() {
                {
                    let mut sq = io_uring.sq();
                    while queue.want() {
                        match sq.next_sqe() {
                            Some(sqe) => {
                                unsafe { black_box(sqe).prep_nop() };
                                queue.pop();
                            },
                            None => break
                        }
                    }
                }

                io_uring.submit_sqes_and_wait(16).unwrap();

                let mut cq = io_uring.cq();
                while let Some(cqe) = cq.peek_for_cqe() {
                    black_box(cqe);
                }
            }
        });
    });
}

criterion_group!(squeue, bench_normal, bench_concurrent, bench_iou);
criterion_main!(squeue);
