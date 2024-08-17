use crate::Test;
use io_uring::types::FutexWaitV;
use io_uring::{cqueue, opcode, squeue, IoUring};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use std::{io, ptr, thread};

// Not defined by libc.
//
// From: https://github.com/torvalds/linux/blob/v6.7/include/uapi/linux/futex.h#L63
const FUTEX2_SIZE_U32: u32 = 2;

const INIT_VAL: u32 = 0xDEAD_BEEF;

fn syscall_futex(futex: *const u32, op: libc::c_int, val: u32) -> io::Result<i64> {
    let ret = unsafe {
        libc::syscall(
            libc::SYS_futex,
            futex,
            op,
            val,
            ptr::null::<u8>(),
            ptr::null::<u8>(),
            0u32,
        )
    };
    if ret >= 0 {
        Ok(ret as _)
    } else {
        Err(io::Error::from_raw_os_error(-ret as _))
    }
}

pub fn test_futex_wait<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::FutexWait::CODE);
    );

    const USER_DATA: u64 = 0xDEAD_BEEF_DEAD_BEEF;

    println!("test futex_wait");

    let mut futex = INIT_VAL;

    let futex_wait_e = opcode::FutexWait::new(
        &futex,
        INIT_VAL as u64,
        // NB. FUTEX_BITSET_MATCH_ANY is signed. We are operating on 32-bit futex, thus this mask
        // must be 32-bit. Converting directly from c_int to u64 will yield `u64::MAX`, which is
        // invalid.
        libc::FUTEX_BITSET_MATCH_ANY as u32 as u64,
        FUTEX2_SIZE_U32,
    );

    unsafe {
        let mut queue = ring.submission();
        queue
            .push(&futex_wait_e.build().user_data(USER_DATA).into())
            .expect("queue is full");
    }

    ring.submit()?;
    thread::sleep(Duration::from_millis(100));
    assert_eq!(ring.completion().len(), 0);

    futex += 1;
    let ret = syscall_futex(&futex, libc::FUTEX_WAKE, 1)?;
    assert_eq!(ret, 1);

    ring.submit_and_wait(1)?;
    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();

    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), USER_DATA);
    assert_eq!(cqes[0].result(), 0);

    Ok(())
}

pub fn test_futex_wake<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::FutexWake::CODE);
    );

    const USER_DATA: u64 = 0xBEEF_DEAD_BEEF_DEAD;

    println!("test futex_wake");

    let futex = Arc::new(AtomicU32::new(INIT_VAL));

    let wait_thread = std::thread::spawn({
        let futex = futex.clone();
        move || {
            while futex.load(Ordering::Relaxed) == INIT_VAL {
                let ret = syscall_futex(futex.as_ptr(), libc::FUTEX_WAIT, INIT_VAL);
                assert_eq!(ret.unwrap(), 0);
            }
        }
    });

    thread::sleep(Duration::from_millis(100));
    assert!(!wait_thread.is_finished());

    futex.store(INIT_VAL + 1, Ordering::Relaxed);

    let futex_wake_e = opcode::FutexWake::new(
        futex.as_ptr(),
        1,
        // NB. See comments above for why it cannot be a single `as u64`.
        libc::FUTEX_BITSET_MATCH_ANY as u32 as u64,
        FUTEX2_SIZE_U32,
    );
    unsafe {
        let mut queue = ring.submission();
        queue
            .push(&futex_wake_e.build().user_data(USER_DATA).into())
            .expect("queue is full");
    }
    ring.submit_and_wait(1)?;
    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), USER_DATA);
    assert_eq!(cqes[0].result(), 1);

    wait_thread.join().unwrap();

    Ok(())
}

pub fn test_futex_waitv<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::FutexWaitV::CODE);
    );

    const USER_DATA: u64 = 0xDEAD_BEEF_BEEF_DEAD;

    println!("test futex_waitv");

    const FUTEX_CNT: usize = 5;
    const TRIGGER_IDX: usize = 3;

    let mut futexes = [INIT_VAL; FUTEX_CNT];
    let mut waitv = [FutexWaitV::default(); FUTEX_CNT];
    for (futex, waitv) in futexes.iter().zip(&mut waitv) {
        *waitv = FutexWaitV::new()
            .val(INIT_VAL as u64)
            .uaddr(std::ptr::from_ref(futex) as _)
            .flags(FUTEX2_SIZE_U32);
    }

    let futex_waitv_e = opcode::FutexWaitV::new(waitv.as_ptr().cast(), waitv.len() as _);
    unsafe {
        let mut queue = ring.submission();
        queue
            .push(&futex_waitv_e.build().user_data(USER_DATA).into())
            .expect("queue is full");
    }

    ring.submit()?;
    thread::sleep(Duration::from_millis(100));
    assert_eq!(ring.completion().len(), 0);

    futexes[TRIGGER_IDX] += 1;
    let ret = syscall_futex(&futexes[TRIGGER_IDX], libc::FUTEX_WAKE, 1)?;
    assert_eq!(ret, 1);

    ring.submit_and_wait(1)?;
    let cqes: Vec<cqueue::Entry> = ring.completion().map(Into::into).collect();
    assert_eq!(cqes.len(), 1);
    assert_eq!(cqes[0].user_data(), USER_DATA);
    assert_eq!(cqes[0].result(), TRIGGER_IDX as _);

    Ok(())
}
