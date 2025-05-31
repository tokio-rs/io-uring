use std::os::fd::AsRawFd;
use anyhow::{bail, Context};
use crate::Test;
use io_uring::{cqueue, opcode, squeue, IoUring};
use io_uring::cqueue::Entry;

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

pub fn test_register_files_tags<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    // register_files_sparse was introduced in kernel 5.13, we need to check if resource tagging
    // is available.
    require!(
        test;
        ring.params().is_feature_resource_tagging();
    );

    println!("test register_files_tags");

    let file1 = tempfile::tempfile()?;
    let file2 = tempfile::tempfile()?;
    let file3 = tempfile::tempfile()?;

    let descriptors = &[
        file1.as_raw_fd(),
        file2.as_raw_fd(),
        file3.as_raw_fd(),
    ];

    let flags = &[0, 1, 2];

    ring.submitter()
        .register_files_tags(descriptors, flags)
        .context("register_files_tags failed")?;

    // See that same call again, with any value, will fail because a direct table cannot be built
    // over an existing one.
    if let Ok(()) = ring.submitter().register_files_tags(descriptors, flags) {
        bail!("register_files_tags should not have succeeded twice in a row");
    }

    // See that the direct table can be removed.
    ring.submitter()
        .unregister_files()
        .context("unregister_files failed")?;

    // See that a second attempt to remove the direct table would fail.
    if let Ok(()) = ring.submitter().unregister_files() {
        bail!("should not have succeeded twice in a row");
    }
    
    // Check that we get completion events for unregistering files with non-zero tags.
    let mut completion = ring.completion();
    completion.sync();
    
    let cqes = completion
        .map(Into::into)
        .collect::<Vec<Entry>>();
    
    if cqes.len() != 2 {
        bail!("incorrect number of completion events: {cqes:?}")
    }
    
    for event in cqes {
        if event.flags() != 0 || event.result() != 0 {
            bail!(
                "completion events from unregistering tagged \
                files should have zeroed flags and result fields"
            );
        }

        let tag = event.user_data();
        if !(1..3).contains(&tag) {
            bail!("completion event user data does not contain one of the possible tag values")
        }
    }

    Ok(())
}

pub fn test_register_files_update_tag<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    // register_files_update_tag was introduced in kernel 5.13, we need to check if resource tagging
    // is available.
    require!(
        test;
        ring.params().is_feature_resource_tagging();
    );
    
    println!("test register_files_update_tag");
    
    let descriptors = &[
        -1,
        -1,
        -1,
    ];    
    let flags = &[0, 0, 0];    
    ring.submitter()
        .register_files_tags(descriptors, flags)
        .context("register_files_tags failed")?;

    let file = tempfile::tempfile()
        .context("create temp file")?;
    
    // Update slot `1` with a tagged file
    ring.submitter()
        .register_files_update_tag(1, &[file.as_raw_fd()], &[1])
        .context("register_files_update_tag failed")?;

    // Update slot `2` with an untagged file
    ring.submitter()
        .register_files_update_tag(1, &[file.as_raw_fd()], &[0])
        .context("register_files_update_tag failed")?;
    
    ring.submitter()
        .unregister_files()
        .context("unregister_files failed")?;

    // Check that we get completion events for unregistering files with non-zero tags.
    let mut completion = ring.completion();
    completion.sync();
    
    let cqes = completion
        .map(Into::into)
        .collect::<Vec<Entry>>();

    if cqes.len() != 1 {
        bail!("incorrect number of completion events: {cqes:?}")
    }
    
    if cqes[0].result() != 0 || cqes[0].flags() != 0 {
        bail!(
            "completion events from unregistering tagged \
            files should have zeroed flags and result fields"
        );
    }

    if cqes[0].user_data() != 1 {
        bail!("completion event user data does not contain tag of registered file");
    }
    
    Ok(())
}
