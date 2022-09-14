#[macro_use]
mod utils;
mod tests;

use io_uring::{cqueue, squeue, IoUring, Probe};

pub struct Test {
    probe: Probe,
    target: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let entries = 8;

    test::<squeue::Entry, cqueue::Entry>(IoUring::new(entries)?)?;

    #[cfg(all(feature = "unstable", not(feature = "ci")))]
    {
        match IoUring::<squeue::Entry128, cqueue::Entry>::generic_new(entries) {
            Ok(r) => test(r)?,
            Err(e) => {
                println!(
                    "IoUring::<squeue::Entry128, cqueue::Entry>::generic_new(entries) failed: {}",
                    e
                );
                println!("Assume kernel doesn't support the new entry sizes so remaining tests being skipped.");
                return Ok(());
            }
        };
        test(IoUring::<squeue::Entry, cqueue::Entry32>::generic_new(
            entries,
        )?)?;
        test(IoUring::<squeue::Entry128, cqueue::Entry32>::generic_new(
            entries,
        )?)?;
    }

    Ok(())
}

fn test<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    mut ring: IoUring<S, C>,
) -> anyhow::Result<()> {
    let mut probe = Probe::new();

    if ring.submitter().register_probe(&mut probe).is_err() {
        eprintln!("No probe supported");
    }

    println!();
    println!(
        "ring type: IoUring<{}, {}>",
        std::any::type_name::<S>()
            .strip_prefix("io_uring::")
            .unwrap(),
        std::any::type_name::<C>()
            .strip_prefix("io_uring::")
            .unwrap(),
    );
    println!("params: {:#?}", ring.params());
    println!("probe: {:?}", probe);
    println!();

    let test = Test {
        probe,
        target: std::env::args().nth(1),
    };

    tests::queue::test_nop(&mut ring, &test)?;
    tests::queue::test_queue_split(&mut ring, &test)?;
    tests::queue::test_debug_print(&mut ring, &test)?;

    #[cfg(feature = "unstable")]
    tests::queue::test_batch(&mut ring, &test)?;

    // fs
    tests::fs::test_file_write_read(&mut ring, &test)?;
    tests::fs::test_file_writev_readv(&mut ring, &test)?;
    tests::fs::test_file_cur_pos(&mut ring, &test)?;
    tests::fs::test_file_fsync(&mut ring, &test)?;
    tests::fs::test_file_fsync_file_range(&mut ring, &test)?;
    tests::fs::test_file_fallocate(&mut ring, &test)?;
    tests::fs::test_file_openat2(&mut ring, &test)?;
    tests::fs::test_file_close(&mut ring, &test)?;
    #[cfg(not(feature = "ci"))]
    tests::fs::test_file_direct_write_read(&mut ring, &test)?;
    #[cfg(not(feature = "ci"))]
    tests::fs::test_statx(&mut ring, &test)?;
    #[cfg(feature = "unstable")]
    tests::fs::test_file_splice(&mut ring, &test)?;

    // timeout
    tests::timeout::test_timeout(&mut ring, &test)?;
    tests::timeout::test_timeout_count(&mut ring, &test)?;
    tests::timeout::test_timeout_remove(&mut ring, &test)?;
    tests::timeout::test_timeout_cancel(&mut ring, &test)?;
    tests::timeout::test_timeout_abs(&mut ring, &test)?;
    #[cfg(feature = "unstable")]
    tests::timeout::test_timeout_submit_args(&mut ring, &test)?;

    // net
    tests::net::test_tcp_write_read(&mut ring, &test)?;
    tests::net::test_tcp_writev_readv(&mut ring, &test)?;
    tests::net::test_tcp_send_recv(&mut ring, &test)?;
    tests::net::test_tcp_sendmsg_recvmsg(&mut ring, &test)?;
    tests::net::test_tcp_accept(&mut ring, &test)?;
    tests::net::test_tcp_connect(&mut ring, &test)?;
    #[cfg(feature = "unstable")]
    tests::net::test_tcp_buffer_select(&mut ring, &test)?;

    // queue
    tests::poll::test_eventfd_poll(&mut ring, &test)?;
    tests::poll::test_eventfd_poll_remove(&mut ring, &test)?;
    tests::poll::test_eventfd_poll_remove_failed(&mut ring, &test)?;

    Ok(())
}
