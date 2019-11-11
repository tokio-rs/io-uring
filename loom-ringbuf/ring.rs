use std::mem::MaybeUninit;
use loom::cell::CausalCell;
use loom::sync::atomic;

const LENGTH: u32 = 8;
const MASK: u32 = LENGTH - 1;


pub struct Ring<T> {
    buf: Box<[CausalCell<MaybeUninit<T>>]>,
    head: atomic::AtomicU32,
    tail: atomic::AtomicU32,
    mark: atomic::AtomicU32
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
            mark: atomic::AtomicU32::new(0)
        }
    }

    pub fn push(&self, t: T) -> Result<(), T> {
        let mark = loop {
            let head = self.head.load(atomic::Ordering::Acquire);
            let mark = self.mark.load(atomic::Ordering::Acquire);

            if mark.wrapping_sub(head) == LENGTH {
                return Err(t);
            }

            if self.mark.compare_and_swap(mark, mark.wrapping_add(1), atomic::Ordering::Release)
                == mark
            {
                break mark;
            }
        };

        unsafe {
            self.buf[(mark & MASK) as usize]
                .with_mut(|p| (*p).as_mut_ptr().write(t));
        }

        while self.tail.compare_and_swap(mark, mark.wrapping_add(1), atomic::Ordering::Release)
            != mark
        {
            atomic::spin_loop_hint();
        }

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

            if self.head.compare_and_swap(head, head.wrapping_add(1), atomic::Ordering::Release)
                == head
            {
                return Some(t);
            }
        }
    }
}
