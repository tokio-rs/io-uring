use io_uring::{cqueue, opcode, squeue, types, IoUring};
use std::fs::File;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use tempfile::tempdir;

use crate::Test;

/// Test to reproduce SQPOLL CQ overflow issue
///
/// This test demonstrates the issue when:
/// 1) SQPOLL is enabled
/// 2) IORING_FEAT_NODROP feature is supported
/// 3) We submit more concurrent I/O requests than the CQ can handle
pub fn test_sqpoll_cq_overflow<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> anyhow::Result<()> {
    if !ring.params().is_feature_nodrop() {
        println!("IORING_FEAT_NODROP is not supported by the kernel, skip");
        return Ok(());
    }

    require! {
        test;
    }

    println!("test sqpoll_cq_overflow");

    let idle = ring.params().sq_thread_idle();
    let entries = ring.params().sq_entries() as usize;
    let num_requests = entries * 4;

    // Create a temporary directory for test files
    let temp_dir = tempdir().expect("Failed to create temp directory");
    let test_dir = temp_dir.path();

    // Create test files for I/O operations
    let test_files = create_test_files(test_dir, num_requests);
    let mut bufs = (0..num_requests)
        .map(|_| vec![0u8; 1024])
        .collect::<Vec<_>>();

    let start = std::time::Instant::now();

    test_files
        .iter()
        .zip(bufs.iter_mut())
        .for_each(|(file, buf)| {
            let fd = file.as_raw_fd();
            let entry = opcode::Read::new(types::Fd(fd), buf.as_mut_ptr(), buf.len() as _)
                .build()
                .into();
            while unsafe { ring.submission().push(&entry).is_err() } {
                ring.submit().expect("Failed to submit");
            }
        });

    let mut completed_count = 0;
    // Try to collect all completions
    while completed_count < num_requests {
        while ring.completion().next().is_some() {
            completed_count += 1;
        }

        if ring.submission().cq_overflow() {
            // Call `io_uring_enter` to make the kernel flush overflowed completions
            println!("CQ overflow, syscall to flush");
            ring.submit().expect("Failed to submit");
        }
    }

    let end = start.elapsed();

    assert_eq!(completed_count, num_requests);
    // After idle time, `IORING_SQ_NEED_WAKEUP` flag will be set, and `io_uring_enter` will be called.
    // That call will flush the overflowed completions eventually.
    // To distinguish this case from the case where the fix of PR #349 is applied,
    // we need to check the elapsed time.
    assert!(
        end.as_millis() < idle as _,
        "Overflown completions should be flushed within the idle time"
    );

    Ok(())
}

/// Create test files for I/O operations
fn create_test_files(dir: &Path, count: usize) -> Vec<File> {
    let mut files = Vec::new();

    for i in 0..count {
        let file_path = dir.join(format!("test_file_{}.txt", i));
        let mut file = File::create(&file_path).expect("Failed to create test file");

        // Write some data to the file
        let content = format!("Test content for file {}", i);
        file.write_all(content.as_bytes())
            .expect("Failed to write to test file");
        file.flush().expect("Failed to flush file");

        // Reopen for reading
        let read_file = File::open(&file_path).expect("Failed to open test file for reading");
        files.push(read_file);
    }

    files
}
