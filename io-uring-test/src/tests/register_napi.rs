use std::io;

use io_uring::cqueue;
use io_uring::squeue;
use io_uring::types::{Napi, NapiTracking};
use io_uring::IoUring;

use crate::Test;

pub fn test_register_napi<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
    test: &Test,
) -> io::Result<()> {
    require!(
        test;
        // IORING_REGISTER_NAPI requires Linux 6.9+.
        napi_supported(ring);
    );

    println!("test register_napi");

    // Register a first configuration. The kernel writes the previous (default) settings
    // back into `first`, so it should report a disabled busy-poll timeout.
    let mut first = Napi::new()
        .set_busy_poll_timeout(60)
        .set_prefer_busy_poll(true);
    ring.submitter().register_napi(&mut first)?;
    assert_eq!(
        first.busy_poll_timeout(),
        0,
        "previous timeout should be unset"
    );

    // Registering again returns the settings we just installed.
    let mut second = Napi::new().set_busy_poll_timeout(120);
    ring.submitter().register_napi(&mut second)?;
    assert_eq!(
        second.busy_poll_timeout(),
        60,
        "register_napi should return the previous busy-poll timeout"
    );
    assert!(
        second.prefer_busy_poll(),
        "register_napi should return the previous prefer_busy_poll setting"
    );

    // Read the current settings back while disabling.
    let mut current = Napi::new();
    ring.submitter().unregister_napi(&mut current)?;
    assert_eq!(
        current.busy_poll_timeout(),
        120,
        "unregister_napi should return the current busy-poll timeout"
    );

    // Static tracking with explicit NAPI id management (Linux 6.13+). On 6.9-6.12 the
    // kernel rejects the static tracking strategy, so guard on the register succeeding.
    let mut static_cfg = Napi::new().set_tracking(NapiTracking::Static);
    if ring.submitter().register_napi(&mut static_cfg).is_ok() {
        // We have no real NAPI id here (loopback exposes none), so we only confirm the
        // calls reach the kernel; a fabricated id is rejected with EINVAL, which is an
        // acceptable outcome for this exercise.
        let _ = ring.submitter().register_napi_add_id(1);
        let _ = ring.submitter().register_napi_del_id(1);
        ring.submitter().unregister_napi(&mut Napi::new())?;
    }

    Ok(())
}

/// Probe whether the kernel supports `IORING_REGISTER_NAPI` by registering and immediately
/// unregistering a default configuration. Returns `false` on kernels older than 6.9.
fn napi_supported<S: squeue::EntryMarker, C: cqueue::EntryMarker>(
    ring: &mut IoUring<S, C>,
) -> bool {
    let mut napi = Napi::new();
    if ring.submitter().register_napi(&mut napi).is_err() {
        return false;
    }
    let _ = ring.submitter().unregister_napi(&mut napi);
    true
}
