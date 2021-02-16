#[macro_use]
mod helper;
mod tests;

#[cfg(feature = "unstable")]
mod ownedsplit;

use io_uring::{IoUring, Probe};

fn main() -> anyhow::Result<()> {
    let mut ring = IoUring::new(8)?;
    let mut probe = Probe::new();

    if ring.submitter().register_probe(&mut probe).is_err() {
        eprintln!("No probe supported");
    }

    tests::queue::test_nop(&mut ring, &probe)?;

    #[cfg(feature = "unstable")]
    tests::queue::test_batch(&mut ring, &probe)?;

    // fs
    tests::fs::test_file_write_read(&mut ring, &probe)?;
    tests::fs::test_file_writev_readv(&mut ring, &probe)?;
    tests::fs::test_file_cur_pos(&mut ring, &probe)?;
    tests::fs::test_file_fsync(&mut ring, &probe)?;
    tests::fs::test_file_fsync_file_range(&mut ring, &probe)?;
    tests::fs::test_file_fallocate(&mut ring, &probe)?;
    tests::fs::test_file_openat2(&mut ring, &probe)?;
    tests::fs::test_file_close(&mut ring, &probe)?;

    // timeout
    tests::timeout::test_timeout(&mut ring, &probe)?;
    tests::timeout::test_timeout_count(&mut ring, &probe)?;
    tests::timeout::test_timeout_remove(&mut ring, &probe)?;
    tests::timeout::test_timeout_cancel(&mut ring, &probe)?;

    // net
    tests::net::test_tcp_write_read(&mut ring, &probe)?;
    tests::net::test_tcp_writev_readv(&mut ring, &probe)?;
    tests::net::test_tcp_accept(&mut ring, &probe)?;
    tests::net::test_tcp_connect(&mut ring, &probe)?;
    tests::net::test_tcp_send_recv(&mut ring, &probe)?;
    tests::net::test_tcp_sendmsg_recvmsg(&mut ring, &probe)?;

    // queue
    tests::poll::test_eventfd_poll(&mut ring, &probe)?;
    tests::poll::test_eventfd_poll_remove(&mut ring, &probe)?;
    tests::poll::test_eventfd_poll_remove_failed(&mut ring, &probe)?;

    #[cfg(feature = "unstable")]
    ownedsplit::main(ring, &probe)?;

    Ok(())
}
