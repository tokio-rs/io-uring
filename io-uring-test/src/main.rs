mod helper;
mod tests;

use io_uring::{opcode, IoUring, Probe};

fn main() -> anyhow::Result<()> {
    let mut ring = IoUring::new(8)?;
    let mut probe = Probe::new();

    if ring.submitter().register_probe(&mut probe).is_err() {
        eprintln!("No probe supported");
    }

    tests::queue::test_nop(&mut ring)?;

    if probe.is_supported(opcode::Write::CODE) && probe.is_supported(opcode::Read::CODE) {
        tests::fs::test_file_write_read(&mut ring)?;
        tests::net::test_tcp_write_read(&mut ring)?;
    }

    if probe.is_supported(opcode::Writev::CODE) && probe.is_supported(opcode::Readv::CODE) {
        tests::fs::test_file_writev_readv(&mut ring)?;
        tests::net::test_tcp_writev_readv(&mut ring)?;
    }

    if probe.is_supported(opcode::Fsync::CODE) {
        tests::fs::test_file_fsync(&mut ring)?;
    }

    if probe.is_supported(opcode::PollAdd::CODE) {
        tests::poll::test_eventfd_poll(&mut ring)?;

        if probe.is_supported(opcode::PollRemove::CODE) {
            tests::poll::test_eventfd_poll_remove(&mut ring)?;
            tests::poll::test_eventfd_poll_remove_failed(&mut ring)?;
        }
    }

    if probe.is_supported(opcode::SyncFileRange::CODE) {
        tests::fs::test_file_fsync_file_range(&mut ring)?;
    }

    if probe.is_supported(opcode::Timeout::CODE) {
        tests::timeout::test_timeout(&mut ring)?;

        if probe.is_supported(opcode::TimeoutRemove::CODE) {
            tests::timeout::test_timeout_remove(&mut ring)?;
            tests::timeout::test_timeout_count(&mut ring)?;
        }
    }

    if probe.is_supported(opcode::Accept::CODE) {
        assert!(probe.is_supported(opcode::Write::CODE) && probe.is_supported(opcode::Read::CODE));

        tests::net::test_tcp_accept(&mut ring)?;
    }

    if probe.is_supported(opcode::Connect::CODE) {
        assert!(probe.is_supported(opcode::Write::CODE) && probe.is_supported(opcode::Read::CODE));

        tests::net::test_tcp_connect(&mut ring)?;
    }

    if probe.is_supported(opcode::Fallocate::CODE) {
        tests::fs::test_file_fallocate(&mut ring)?;
    }

    if probe.is_supported(opcode::Openat2::CODE) {
        tests::fs::test_file_openat2(&mut ring)?;
    }

    if probe.is_supported(opcode::Close::CODE) {
        tests::fs::test_file_close(&mut ring)?;
    }

    if probe.is_supported(opcode::Send::CODE) && probe.is_supported(opcode::Recv::CODE) {
        tests::net::test_tcp_send_recv(&mut ring)?;
    }

    if probe.is_supported(opcode::SendMsg::CODE) && probe.is_supported(opcode::RecvMsg::CODE) {
        tests::net::test_tcp_sendmsg_recvmsg(&mut ring)?;
    }

    Ok(())
}
