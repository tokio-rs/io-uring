use std::io;
use std::os::unix::io::AsRawFd;
use tempfile::tempfile;
use criterion::{ criterion_main, criterion_group, Criterion, black_box };
use io_uring::{ opcode, IoUring };


fn bench_writev(c: &mut Criterion) {
    let mut ring = IoUring::new(16).unwrap();
    let fd = tempfile().unwrap();

    let buf = vec![0x00; 32];
    let buf2 = vec![0x01; 33];
    let buf3 = vec![0x02; 34];
    let buf4 = vec![0x03; 35];
    let buf5 = vec![0x04; 36];
    let bufs = [
        io::IoSlice::new(&buf),
        io::IoSlice::new(&buf2),
        io::IoSlice::new(&buf3),
        io::IoSlice::new(&buf4),
        io::IoSlice::new(&buf5),
    ];

    c.bench_function("writev", |b| {
        b.iter(|| {
            let bufs = black_box(&bufs);

            let entry = opcode::Writev::new(
                fd.as_raw_fd().into(),
                bufs.as_ptr() as *const _,
                bufs.len() as _
            );

            unsafe {
                ring.submission()
                    .available()
                    .push(entry.build())
                    .ok()
                    .expect("queue is full");
            }

            ring.submit_and_wait(1).unwrap();
            ring
                .completion()
                .available()
                .map(black_box)
                .for_each(drop);
        });
    });
}

#[cfg(not(feature = "unstable"))]
fn bench_writes(_: &mut Criterion) {}

#[cfg(feature = "unstable")]
fn bench_writes(c: &mut Criterion) {
    let mut ring = IoUring::new(16).unwrap();
    let fd = tempfile().unwrap();

    let buf = vec![0x00; 32];
    let buf2 = vec![0x01; 33];
    let buf3 = vec![0x02; 34];
    let buf4 = vec![0x03; 35];
    let buf5 = vec![0x04; 36];
    let bufs = [
        &buf[..],
        &buf2[..],
        &buf3[..],
        &buf4[..],
        &buf5[..],
    ];

    c.bench_function("one-submit", |b| {
        b.iter(|| {
            let mut queue = ring.submission().available();
            for buf in black_box(&bufs) {
                let entry = opcode::Write::new(
                    fd.as_raw_fd().into(),
                    buf.as_ptr(),
                    buf.len() as _
                );

                unsafe {
                    queue
                        .push(entry.build())
                        .ok()
                        .expect("queue is full");
                }
            }

            drop(queue);

            ring.submit_and_wait(bufs.len()).unwrap();
            ring
                .completion()
                .available()
                .map(black_box)
                .for_each(drop);
        });
    });

    c.bench_function("multi-submit", |b| {
        b.iter(|| {
            for buf in black_box(&bufs) {
                let entry = opcode::Write::new(
                    fd.as_raw_fd().into(),
                    buf.as_ptr(),
                    buf.len() as _
                );

                unsafe {
                    ring.submission()
                        .available()
                        .push(entry.build())
                        .ok()
                        .expect("queue is full");
                }

                ring.submit_and_wait(1).unwrap();
                ring
                    .completion()
                    .available()
                    .map(black_box)
                    .for_each(drop);
            }
        });
    });
}

criterion_group!(iovec, bench_writev, bench_writes);
criterion_main!(iovec);
