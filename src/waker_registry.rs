use crate::locked_waker::*;
use crossbeam::queue::{ArrayQueue, SegQueue};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::task::Context;

#[enum_dispatch(RegistryTrait)]
pub enum Registry {
    Single(RegistrySingle),
    Multi(RegistryMulti),
    Dummy(RegistryDummy),
}

#[enum_dispatch]
pub trait RegistryTrait {
    /// For async context
    fn reg_async(&self, _ctx: &mut Context, _o_waker: &mut Option<LockedWaker>) -> bool;

    /// For thread context
    fn reg_blocking(&self, _waker: &LockedWaker);

    fn clear_wakers(&self, _seq: u64);

    fn cancel_waker(&self, _waker: LockedWaker);

    fn fire(&self);

    fn close(&self);

    /// return waker queue size
    fn get_size(&self) -> usize;
}

/// RegistryDummy is for unbounded channel tx, which is never blocked
pub struct RegistryDummy();

impl RegistryDummy {
    #[inline(always)]
    pub fn new() -> Registry {
        Registry::Dummy(RegistryDummy())
    }
}

impl RegistryTrait for RegistryDummy {
    #[inline(always)]
    fn reg_async(&self, _ctx: &mut Context, _o_waker: &mut Option<LockedWaker>) -> bool {
        unreachable!();
    }

    #[inline(always)]
    fn reg_blocking(&self, _waker: &LockedWaker) {
        unreachable!();
    }

    #[inline(always)]
    fn clear_wakers(&self, _seq: u64) {}

    #[inline(always)]
    fn cancel_waker(&self, _waker: LockedWaker) {}

    #[inline(always)]
    fn fire(&self) {}

    #[inline(always)]
    fn close(&self) {}

    /// return waker queue size
    #[inline(always)]
    fn get_size(&self) -> usize {
        0
    }
}

pub struct RegistrySingle {
    queue: ArrayQueue<LockedWakerRef>,
}

impl RegistrySingle {
    #[inline(always)]
    pub fn new() -> Registry {
        Registry::Single(Self { queue: ArrayQueue::new(1) })
    }
}

impl RegistryTrait for RegistrySingle {
    /// return is_skip
    #[inline(always)]
    fn reg_async(&self, ctx: &mut Context, o_waker: &mut Option<LockedWaker>) -> bool {
        let waker = {
            if o_waker.is_none() {
                o_waker.replace(LockedWaker::new_async(ctx));
                o_waker.as_ref().unwrap()
            } else {
                let _waker = o_waker.as_ref().unwrap();
                if !_waker.is_waked() {
                    // No need to reg again, since waker is not consumed
                    return true;
                }
                _waker
            }
        };
        match self.queue.push(waker.weak()) {
            Ok(_) => {}
            Err(_weak) => {
                let _old_waker = self.queue.pop();
                self.queue.push(_weak).expect("push wake ok after pop");
            }
        }
        false
    }

    #[inline(always)]
    fn reg_blocking(&self, waker: &LockedWaker) {
        match self.queue.push(waker.weak()) {
            Ok(_) => {}
            Err(_weak) => {
                let _old_waker = self.queue.pop();
                self.queue.push(_weak).expect("push wake ok after pop");
            }
        }
    }

    #[inline(always)]
    fn cancel_waker(&self, _waker: LockedWaker) {
        // Got to be it, because only one single thread.
        let _ = self.queue.pop();
    }

    #[inline(always)]
    fn clear_wakers(&self, _seq: u64) {
        // Got to be it, because only one single thread.
        let _ = self.queue.pop();
    }

    #[inline(always)]
    fn fire(&self) {
        if let Some(waker) = self.queue.pop() {
            waker.wake();
        }
    }

    #[inline(always)]
    fn close(&self) {
        self.fire();
    }

    /// return waker queue size
    #[inline(always)]
    fn get_size(&self) -> usize {
        self.queue.len()
    }
}

pub struct RegistryMulti {
    queue: SegQueue<LockedWakerRef>,
    seq: AtomicU64,
    checking: AtomicBool,
}

impl RegistryMulti {
    #[inline(always)]
    pub fn new() -> Registry {
        Registry::Multi(Self {
            queue: SegQueue::new(),
            seq: AtomicU64::new(0),
            checking: AtomicBool::new(false),
        })
    }
}

impl RegistryTrait for RegistryMulti {
    #[inline(always)]
    fn reg_async(&self, ctx: &mut Context, o_waker: &mut Option<LockedWaker>) -> bool {
        let waker = {
            if o_waker.is_none() {
                o_waker.replace(LockedWaker::new_async(ctx));
                o_waker.as_ref().unwrap()
            } else {
                let _waker = o_waker.as_ref().unwrap();
                if !_waker.is_waked() {
                    // No need to reg again, since waker is not consumed
                    return true;
                }
                _waker
            }
        };
        waker.set_seq(self.seq.fetch_add(1, Ordering::SeqCst));
        self.queue.push(waker.weak());
        false
    }

    #[inline(always)]
    fn reg_blocking(&self, waker: &LockedWaker) {
        self.queue.push(waker.weak());
    }

    #[inline(always)]
    fn cancel_waker(&self, waker: LockedWaker) {
        // Just abandon and leave it to fire() to clean it
        waker.cancel();
    }

    /// Call when ReceiveFuture is cancelled.
    /// to clear the LockedWakerRef which has been sent to the other side.
    #[inline(always)]
    fn clear_wakers(&self, seq: u64) {
        if self.checking.swap(true, Ordering::SeqCst) {
            // Other thread is cleaning
            return;
        }
        while let Some(waker_ref) = self.queue.pop() {
            if waker_ref.try_to_clear(seq) {
                // we do not known push back may have concurrent problem
                break;
            }
        }
        self.checking.store(false, Ordering::Release);
    }

    #[inline(always)]
    fn fire(&self) {
        while let Some(waker) = self.queue.pop() {
            if waker.wake() {
                return;
            }
        }
    }

    #[inline(always)]
    fn close(&self) {
        while let Some(waker) = self.queue.pop() {
            waker.wake();
        }
    }

    /// return waker queue size
    #[inline(always)]
    fn get_size(&self) -> usize {
        self.queue.len()
    }
}
