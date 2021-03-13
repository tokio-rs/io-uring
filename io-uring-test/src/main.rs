#[macro_use]
mod utils;
mod tests;

#[cfg(feature = "unstable")]
mod ownedsplit;

use io_uring::{IoUring, Probe};

pub struct Test {
    probe: Probe,
    target: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let mut ring = IoUring::new(8)?;
    let mut probe = Probe::new();

    if ring.submitter().register_probe(&mut probe).is_err() {
        eprintln!("No probe supported");
    }

    println!("params: {:#?}", ring.params());
    println!("probe: {:?}", probe);
    println!();

    let test = Test {
        probe,
        target: std::env::args().nth(1),
    };

    tests::queue::test_nop(&mut ring, &test)?;

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

    #[cfg(feature = "unstable")]
    ownedsplit::main(ring, &test)?;

    Ok(())
}
