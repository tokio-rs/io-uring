use std::mem::MaybeUninit;
use loom::cell::CausalCell;
use loom::sync::{ atomic, Mutex };

const LENGTH: u32 = 8;
const MASK: u32 = LENGTH - 1;


pub struct Ring<T> {
    buf: Box<[CausalCell<MaybeUninit<T>>]>,
    head: atomic::AtomicU32,
    tail: atomic::AtomicU32,
    push_lock: Mutex<()>
}

impl<T: Copy> Ring<T> {
    pub fn new() -> Ring<T> {
        let mut buf = Vec::with_capacity(LENGTH as usize);

        for _ in 0..LENGTH {
            buf.push(CausalCell::new(MaybeUninit::uninit()));
        }

        Ring {
            buf: buf.into_boxed_slice(),
            head: atomic::AtomicU32::new(0),
            tail: atomic::AtomicU32::new(0),
            push_lock: Mutex::new(())
        }
    }

    pub fn push(&self, t: T) -> Result<(), T> {
        let _lock = self.push_lock.lock().unwrap();
        let head = self.head.load(atomic::Ordering::Acquire);
        let tail = unsafe { self.tail.unsync_load() };

        if tail.wrapping_sub(head) == LENGTH {
            return Err(t);
        }

        unsafe {
            self.buf[(tail & MASK) as usize]
                .with_mut(|p| (*p).as_mut_ptr().write(t));
        }

        self.tail.store(tail.wrapping_add(1), atomic::Ordering::Release);

        Ok(())
    }

    pub fn pop(&self) -> Option<T> {
        loop {
            let head = self.head.load(atomic::Ordering::Acquire);
            let tail = self.tail.load(atomic::Ordering::Acquire);

            if head == tail {
                return None;
            }

            let t = self.buf[(head & MASK) as usize]
                .with(|p| unsafe { (*p).as_ptr().read() });

            match self.head.compare_exchange_weak(
                head,
                head.wrapping_add(1),
                atomic::Ordering::Release,
                atomic::Ordering::Relaxed
            ) {
                Ok(_) => return Some(t),
                Err(_) => ()
            }

            atomic::spin_loop_hint();
        }
    }
}
