use std::{io, os::fd::AsRawFd};

use io_uring::{IoUring, Probe};

const NUM_BUFS: u16 = 8;

fn main() -> io::Result<()> {
    let mut ring = IoUring::new(256)?;

    let mut probe = Probe::default();   
    ring.submitter().register_probe(&mut probe)?;

    
    println!("{probe:?}");

    // let submitter = ring.submitter();

    // let mut buf_ring: Vec<u8> = vec![0; 1024];

    

    // let ring_addr = unsafe {
    //     let page_size = libc::sysconf(libc::_SC_PAGESIZE) as usize;
    //     let buf_ring_ptr = &mut buf_ring.as_mut_ptr() as *mut *mut _ as *mut *mut libc::c_void;
    //     libc::posix_memalign(buf_ring_ptr, page_size, 0);
    // };

    // let ring_addr = buf_ring.as_ptr() as u64;

    // submitter.register_buf_ring(ring_addr, NUM_BUFS, 1)?;
    Ok(())
}
