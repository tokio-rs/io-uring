use std::io;

use crate::Test;
use io_uring::{cqueue, opcode, squeue, IoUring};

pub fn test_register_files_sparse<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    // register_files_sparse was introduced in kernel 5.19, as was the opcode for UringCmd16.
    // So require the UringCmd16 to avoid running this test on earlier kernels.
    require!(
        test;
        test.probe.is_supported(opcode::UringCmd16::CODE);
    );

    println!("test register_files_sparse");

    ring.submitter().register_files_sparse(4)?;

    // See that same call again, with any value, will fail because a direct table cannot be built
    // over an existing one.

    if let Ok(()) = ring.submitter().register_files_sparse(3) {
        return Err(anyhow::anyhow!(
            "register_files_sparse should not have succeeded twice in a row"
        ));
    }

    // See that the direct table can be removed.

    if let Err(e) = ring.submitter().unregister_files() {
        return Err(anyhow::anyhow!("unrgister_files failed: {}", e));
    }

    // See that a second attempt to remove the direct table would fail.

    if let Ok(()) = ring.submitter().unregister_files() {
        return Err(anyhow::anyhow!(
            "unrgister_files should not have succeeded twice in a row"
        ));
    }

    // See that a new, large, direct table can be created.
    // If it fails with EMFILE, print the ulimit command for changing this.

    if let Err(e) = ring.submitter().register_files_sparse(10_000) {
        if let Some(raw_os_err) = e.raw_os_error() {
            if raw_os_err == libc::EMFILE {
                println!(
                    "could not open 10,000 file descriptors, try `ulimit -Sn 11000` in the shell"
                );
                return Ok(());
            } else {
                return Err(anyhow::anyhow!("register_files_sparse should have succeeded after the previous one was removed: {}", e));
            }
        } else {
            return Err(anyhow::anyhow!("register_files_sparse should have succeeded after the previous one was removed: {}", e));
        }
    }

    // And removed.

    if let Err(e) = ring.submitter().unregister_files() {
        return Err(anyhow::anyhow!(
            "unrgister_files failed, odd since the one could be unregistered earlier: {}",
            e
        ));
    }

    Ok(())
}

pub fn test_register_buffers<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    test_register("register_buffers_sparse", ring, test, |ring| {
        ring.submitter().register_buffers_sparse(4)
    })?;
    test_register("register_buffers", ring, test, |ring| {
        let buf = [0xde, 0xed, 0xbe, 0xef];

        unsafe {
            ring.submitter().register_buffers(&[libc::iovec {
                iov_base: buf.as_ptr() as _,
                iov_len: buf.len(),
            }])
        }
    })?;
    test_register("register_buffers_tags", ring, test, |ring| {
        let buf = [0xde, 0xed, 0xbe, 0xef];

        unsafe {
            ring.submitter().register_buffers_tags(
                &[libc::iovec {
                    iov_base: buf.as_ptr() as _,
                    iov_len: buf.len(),
                }],
                &[0],
            )
        }
    })?;

    return Ok(());

    fn test_register<
        S: squeue::EntryMarker,
        C: cqueue::EntryMarker,
        F: FnMut(&mut IoUring<S, C>) -> io::Result<()>,
    >(
        name: &str,
        ring: &mut IoUring<S, C>,
        test: &Test,
        mut register: F,
    ) -> anyhow::Result<()> {
        // register_files_sparse was introduced in kernel 5.19, as was the opcode for UringCmd16.
        // So require the UringCmd16 to avoid running this test on earlier kernels.
        require!(
            test;
            test.probe.is_supported(opcode::UringCmd16::CODE);
        );

        println!("test {name}");

        if let Ok(()) = ring.submitter().unregister_buffers() {
            return Err(anyhow::anyhow!(
                "unregister_buffers should fail if not buffer table has been setup"
            ));
        }

        if let Err(e) = register(ring) {
            return Err(anyhow::anyhow!("{name} failed: {}", e));
        }

        // See that same call again, with any value, will fail because a direct table cannot be built
        // over an existing one.

        if let Ok(()) = register(ring) {
            return Err(anyhow::anyhow!(
                "{name} should not have succeeded twice in a row"
            ));
        }

        // See that the direct table can be removed.

        if let Err(e) = ring.submitter().unregister_buffers() {
            return Err(anyhow::anyhow!("unregister_buffers failed: {}", e));
        }

        // See that a second attempt to remove the direct table would fail.

        if let Ok(()) = ring.submitter().unregister_buffers() {
            return Err(anyhow::anyhow!(
                "unregister_buffers should not have succeeded twice in a row"
            ));
        }

        Ok(())
    }
}
