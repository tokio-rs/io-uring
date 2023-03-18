use std::{
    fs::File,
    io::{self, Error, Write},
    os::fd::{AsRawFd, FromRawFd},
};

use crate::Test;
use io_uring::{cqueue, opcode, squeue, types, IoUring};
use libc::EFAULT;

pub fn test_register_buffers<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    let mut buf = [0xde, 0xed, 0xbe, 0xef];
    let tags = [0];
    let iovecs = [libc::iovec {
        iov_len: buf.len(),
        iov_base: buf.as_mut_ptr() as _,
    }];

    test_register(ring, test, "register_buffers", None, |ring| {
        ring.submitter().register_buffers(&iovecs)
    })?;
    test_register(
        ring,
        test,
        "register_buffers_tags",
        ring.params().is_feature_resource_tagging(),
        |ring| unsafe { ring.submitter().register_buffers_tags(&iovecs, &tags) },
    )?;
    test_register(
        ring,
        test,
        "register_buffers_sparse",
        ring.params().is_feature_resource_tagging(),
        |ring| ring.submitter().register_buffers_sparse(4),
    )?;

    return Ok(());

    fn test_register<
        S: squeue::EntryMarker,
        C: cqueue::EntryMarker,
        F: FnMut(&mut IoUring<S, C>) -> io::Result<()>,
        P: Into<Option<bool>>,
    >(
        ring: &mut IoUring<S, C>,
        test: &Test,
        name: &str,
        probe: P,
        mut register: F,
    ) -> anyhow::Result<()> {
        require!(
            test;
            test.probe.is_supported(opcode::ReadFixed::CODE);
            probe.into().unwrap_or(true);
        );

        println!("test {name}");

        ring.submitter().unregister_buffers().err().ok_or_else(|| {
            anyhow::anyhow!("unregister_buffers should fail if not buffer table has been setup")
        })?;

        register(ring).map_err(|e| anyhow::anyhow!("{name} failed: {e}"))?;

        // See that same call again, with any value, will fail because a direct table cannot be built
        // over an existing one.
        register(ring)
            .err()
            .ok_or_else(|| anyhow::anyhow!("{name} should not have succeeded twice in a row"))?;

        // See that the direct table can be removed.
        ring.submitter()
            .unregister_buffers()
            .map_err(|e| anyhow::anyhow!("unregister_buffers failed: {e}"))?;

        // See that a second attempt to remove the direct table would fail.
        ring.submitter().unregister_buffers().err().ok_or_else(|| {
            anyhow::anyhow!("unregister_buffers should not have succeeded twice in a row")
        })?;

        Ok(())
    }
}

const BUFFER_TAG: u64 = 0xbadcafe;
const TIMEOUT_TAG: u64 = 0xbadf00d;

pub fn test_register_buffers_update_tag<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    require!(
        test;
        test.probe.is_supported(opcode::ReadFixed::CODE);
        ring.params().is_feature_resource_tagging();
    );

    println!("test register_buffers_update_tag");

    let (read, mut write) = create_pipe()?;
    let mut buf = [0xde, 0xed, 0xbe, 0xef];
    let mut tags = [BUFFER_TAG];
    let iovecs = [libc::iovec {
        iov_len: buf.len(),
        iov_base: buf.as_mut_ptr() as _,
    }];
    let timeout = types::Timespec::new().nsec(50 * 1_000_000);
    let timeout = opcode::Timeout::new(&timeout as _)
        .build()
        .user_data(TIMEOUT_TAG)
        .into();
    let read_sqe = opcode::ReadFixed::new(
        types::Fd(read.as_raw_fd()),
        buf.as_mut_ptr(),
        buf.len() as _,
        5,
    )
    .build()
    .user_data(42)
    .into();

    // Register a buffer table and then immediately unregister it
    ring.submitter().register_buffers_sparse(1)?;
    ring.submitter().unregister_buffers()?;

    // Push a timeout of 50ms
    unsafe { ring.submission().push(&timeout).unwrap() };

    // We should not receive any other entries than the timeout
    check_only_timeout(ring)?;

    // Register a sparse buffer table of 10 elements
    ring.submitter().register_buffers_sparse(10)?;

    // Try read the pipe using a sparse buffer
    let cqe = {
        write.write("yo".as_bytes())?;

        unsafe { ring.submission().push(&read_sqe).unwrap() };

        ring.submit_and_wait(1)?;

        ring.completion().next().unwrap().into()
    };

    // We should get the correct user_data
    if cqe.user_data() != 42 {
        return Err(anyhow::anyhow!("unexpected completion queue entry"));
    }

    // EFAULT is to be expected with incorrect fixed buffers
    if cqe.result() != -EFAULT {
        return Err(anyhow::anyhow!("unexpected read result: {}", cqe.result()));
    }

    // Register a buffer at the index 5
    unsafe {
        ring.submitter()
            .register_buffers_update_tag(5, &iovecs, &tags)?;

        // Push a timeout of 50ms
        ring.submission().push(&timeout).unwrap();
    }

    // We should not receive any other entries than the timeout
    check_only_timeout(ring)?;

    // Register a buffer at the same index 5, but this time with an empty tag.
    let cqe = {
        tags[0] = 0;

        unsafe {
            ring.submitter()
                .register_buffers_update_tag(5, &iovecs, &tags)?;
        }
        ring.submit_and_wait(1)?;

        ring.completion().next().unwrap().into()
    };

    // We should get a cqe with the first tag because we registered a
    // new buffer at an index where a buffer was already registered.
    if cqe.user_data() != BUFFER_TAG {
        return Err(anyhow::anyhow!(
            "expected completion queue to contain a buffer unregistered event"
        ));
    }

    // Try reading now that the buffer is registered at index 5
    let cqe = {
        unsafe {
            ring.submission().push(&read_sqe).unwrap();
        }

        ring.submit_and_wait(1)?;

        ring.completion().next().unwrap().into()
    };

    // We should get the correct user_data
    if cqe.user_data() != 42 {
        return Err(anyhow::anyhow!("unexpected completion queue entry"));
    }

    // We should read exactly two bytes
    if cqe.result() != 2 {
        return Err(anyhow::anyhow!("unexpected read result: {}", cqe.result()));
    }

    // The first two bytes of `buf` should be "yo"
    if &buf[0..2] != "yo".as_bytes() {
        return Err(anyhow::anyhow!("unexpected read buffer data: {:x?}", &buf));
    }

    return Ok(());
}

/// Create a pipe and return both ends as RAII `File` handles
fn create_pipe() -> io::Result<(File, File)> {
    let mut fds = [-1, -1];

    unsafe {
        if libc::pipe(fds.as_mut_ptr()) == -1 {
            Err(Error::last_os_error())
        } else {
            Ok((File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])))
        }
    }
}

/// Submit sqes and asserts the only cqe is a timeout entry
fn check_only_timeout<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
) -> Result<(), anyhow::Error> {
    ring.submit_and_wait(1)?;

    if Into::<cqueue::Entry>::into(ring.completion().next().unwrap()).user_data() == TIMEOUT_TAG {
        // There should not be any more entries in the queue
        if ring.completion().next().is_none() {
            return Ok(());
        }
    }

    Err(anyhow::anyhow!("unexpected completion queue entry"))
}
