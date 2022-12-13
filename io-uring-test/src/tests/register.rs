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
