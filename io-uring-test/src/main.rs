#[macro_use]
mod utils;
mod tests;

use io_uring::{cqueue, squeue, IoUring, Probe};
use std::cell::Cell;

pub struct Test {
    probe: Probe,
    target: Option<String>,
    count: Cell<usize>,
}

fn main() -> anyhow::Result<()> {
    let entries = 8;

    test::<squeue::Entry, cqueue::Entry>(IoUring::new(entries)?)?;

    #[cfg(not(feature = "ci"))]
    {
        match IoUring::<squeue::Entry128, cqueue::Entry>::builder().build(entries) {
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
        test(IoUring::<squeue::Entry, cqueue::Entry32>::builder().build(entries)?)?;
        test(IoUring::<squeue::Entry128, cqueue::Entry32>::builder().build(entries)?)?;
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
        count: Cell::new(0),
    };

    tests::queue::test_nop(&mut ring, &test)?;
    tests::queue::test_queue_split(&mut ring, &test)?;
    tests::queue::test_debug_print(&mut ring, &test)?;
    tests::queue::test_msg_ring_data(&mut ring, &test)?;
    tests::queue::test_msg_ring_send_fd(&mut ring, &test)?;

    tests::queue::test_batch(&mut ring, &test)?;

    // register
    tests::register::test_register_files_sparse(&mut ring, &test)?;
    tests::register_buffers::test_register_buffers(&mut ring, &test)?;
    tests::register_buffers::test_register_buffers_update(&mut ring, &test)?;
    tests::register_buf_ring::test_register_buf_ring(&mut ring, &test)?;
    tests::register_sync_cancel::test_register_sync_cancel(&mut ring, &test)?;
    tests::register_sync_cancel::test_register_sync_cancel_unsubmitted(&mut ring, &test)?;
    tests::register_sync_cancel::test_register_sync_cancel_any(&mut ring, &test)?;

    // async cancellation
    tests::cancel::test_async_cancel_user_data(&mut ring, &test)?;
    tests::cancel::test_async_cancel_user_data_all(&mut ring, &test)?;
    tests::cancel::test_async_cancel_any(&mut ring, &test)?;
    tests::cancel::test_async_cancel_fd(&mut ring, &test)?;
    tests::cancel::test_async_cancel_fd_all(&mut ring, &test)?;

    // epoll
    tests::epoll::test_ready(&mut ring, &test)?;
    tests::epoll::test_not_ready(&mut ring, &test)?;
    tests::epoll::test_delete(&mut ring, &test)?;
    tests::epoll::test_remove(&mut ring, &test)?;
    tests::epoll::test_race(&mut ring, &test)?;

    // fs
    tests::fs::test_file_write_read(&mut ring, &test)?;
    tests::fs::test_pipe_read_multishot(&mut ring, &test)?;
    tests::fs::test_file_writev_readv(&mut ring, &test)?;
    tests::fs::test_pipe_fixed_writev_readv(&mut ring, &test)?;
    tests::fs::test_file_cur_pos(&mut ring, &test)?;
    tests::fs::test_file_fsync(&mut ring, &test)?;
    tests::fs::test_file_fsync_file_range(&mut ring, &test)?;
    tests::fs::test_file_fallocate(&mut ring, &test)?;
    tests::fs::test_file_openat2(&mut ring, &test)?;
    tests::fs::test_file_openat2_close_file_index(&mut ring, &test)?;
    tests::fs::test_file_openat_close_file_index(&mut ring, &test)?;
    tests::fs::test_file_close(&mut ring, &test)?;
    tests::fs::test_file_direct_write_read(&mut ring, &test)?;
    #[cfg(not(feature = "ci"))]
    tests::fs::test_statx(&mut ring, &test)?;
    tests::fs::test_file_splice(&mut ring, &test)?;
    tests::fs::test_ftruncate(&mut ring, &test)?;
    tests::fs::test_fixed_fd_install(&mut ring, &test)?;
    tests::fs::test_get_set_xattr(&mut ring, &test)?;
    tests::fs::test_f_get_set_xattr(&mut ring, &test)?;

    // timeout
    tests::timeout::test_timeout(&mut ring, &test)?;
    tests::timeout::test_timeout_count(&mut ring, &test)?;
    tests::timeout::test_timeout_remove(&mut ring, &test)?;
    tests::timeout::test_timeout_update(&mut ring, &test)?;
    tests::timeout::test_timeout_cancel(&mut ring, &test)?;
    tests::timeout::test_timeout_abs(&mut ring, &test)?;
    tests::timeout::test_timeout_submit_args(&mut ring, &test)?;
    tests::timeout::test_timeout_multishot(&mut ring, &test)?;

    // net
    tests::net::test_tcp_write_read(&mut ring, &test)?;
    tests::net::test_tcp_writev_readv(&mut ring, &test)?;
    tests::net::test_tcp_send_recv(&mut ring, &test)?;
    tests::net::test_tcp_send_bundle(&mut ring, &test)?;
    tests::net::test_tcp_zero_copy_send_recv(&mut ring, &test)?;
    tests::net::test_tcp_zero_copy_send_fixed(&mut ring, &test)?;
    tests::net::test_tcp_sendmsg_recvmsg(&mut ring, &test)?;
    tests::net::test_tcp_zero_copy_sendmsg_recvmsg(&mut ring, &test)?;
    tests::net::test_tcp_accept(&mut ring, &test)?;
    tests::net::test_tcp_accept_file_index(&mut ring, &test)?;
    tests::net::test_tcp_accept_multi(&mut ring, &test)?;
    tests::net::test_tcp_accept_multi_file_index(&mut ring, &test)?;
    tests::net::test_tcp_connect(&mut ring, &test)?;
    tests::net::test_tcp_buffer_select(&mut ring, &test)?;
    tests::net::test_tcp_buffer_select_recvmsg(&mut ring, &test)?;
    tests::net::test_tcp_buffer_select_readv(&mut ring, &test)?;
    tests::net::test_tcp_recv_multi(&mut ring, &test)?;
    tests::net::test_tcp_recv_bundle(&mut ring, &test)?;
    tests::net::test_tcp_recv_multi_bundle(&mut ring, &test)?;

    tests::net::test_tcp_shutdown(&mut ring, &test)?;
    tests::net::test_socket(&mut ring, &test)?;
    tests::net::test_socket_bind_listen(&mut ring, &test)?;
    tests::net::test_udp_recvmsg_multishot(&mut ring, &test)?;
    tests::net::test_udp_recvmsg_multishot_trunc(&mut ring, &test)?;
    tests::net::test_udp_send_with_dest(&mut ring, &test)?;
    tests::net::test_udp_sendzc_with_dest(&mut ring, &test)?;

    // NOTE: `cqueue::Entry32` required for RecvZC.
    tests::net::test_tcp_recvzc::<S>(&test)?;

    // queue
    tests::poll::test_eventfd_poll(&mut ring, &test)?;
    tests::poll::test_eventfd_poll_remove(&mut ring, &test)?;
    tests::poll::test_eventfd_poll_remove_failed(&mut ring, &test)?;
    tests::poll::test_eventfd_poll_multi(&mut ring, &test)?;

    // futex
    tests::futex::test_futex_wait(&mut ring, &test)?;
    tests::futex::test_futex_wake(&mut ring, &test)?;
    tests::futex::test_futex_waitv(&mut ring, &test)?;

    // os (process)
    tests::os::test_waitid(&mut ring, &test)?;

    // regression test
    tests::regression::test_issue154(&mut ring, &test)?;

    println!("Test count: {}", test.count.get());

    Ok(())
}
