use std::sync::{Arc, Weak, atomic::{AtomicBool, Ordering}};
use std::task::*;

pub struct LockedWaker (Arc<LockedWakerInner>);

struct LockedWakerInner {
    locked: AtomicBool,
    waker: std::task::Waker,
    waked: AtomicBool,
    seq: u64,
}

impl Clone for LockedWaker {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

pub struct LockedWakerRef (Weak<LockedWakerInner>);

impl LockedWaker {

    #[inline(always)]
    pub(crate) fn new(ctx: &Context, seq: u64) -> Self {
        let s = Arc::new(LockedWakerInner{
            seq: seq,
            locked: AtomicBool::new(true), // initial locked
            waker: ctx.waker().clone(),
            waked: AtomicBool::new(false),
        });
        Self(s)
    }

    #[inline(always)]
    pub(crate) fn commit(&self) {
        self.0.locked.store(false, Ordering::Release);
    }

    #[inline(always)]
    pub(crate) fn get_seq(&self) -> u64 {
        self.0.seq
    }

    #[inline(always)]
    pub(crate) fn cancel(&self) {
        let _self = self.0.as_ref();
        _self.waked.store(true, Ordering::Release);
        _self.locked.store(false, Ordering::Release);
    }

    // return is_already waked
    pub(crate) fn abandon(&self) -> bool {
        let _self = self.0.as_ref();
        if _self.waked.load(Ordering::Relaxed) {
            return true
        }
        while _self.locked.swap(true, Ordering::SeqCst) {
            std::hint::spin_loop();
        }
        let r = _self.waked.swap(true, Ordering::SeqCst);
        _self.locked.store(false, Ordering::Release);
        r
    }

    #[inline(always)]
    pub(crate) fn weak(&self) -> LockedWakerRef {
        LockedWakerRef(Arc::downgrade(&self.0))
    }

    #[inline]
    pub fn is_waked(&self) -> bool {
        self.0.waked.load(Ordering::Acquire)
    }

    #[inline(always)]
    pub fn is_canceled(&self) -> bool {
        self.0.waked.load(Ordering::Acquire)
    }

    #[inline]
    pub(crate) fn wake(&self) -> bool {
        let _self = self.0.as_ref();
        while _self.locked.swap(true, Ordering::SeqCst) {
            std::hint::spin_loop();
        }
        if _self.waked.swap(true, Ordering::SeqCst) == false {
            _self.waker.wake_by_ref();
            _self.locked.store(false, Ordering::Release);
            return true;
        }
        _self.locked.store(false, Ordering::Release);
        return false
    }
}

impl LockedWakerRef {

    #[inline(always)]
    pub(crate) fn wake(&self) -> bool {
        if let Some(_self) = self.0.upgrade() {
            return LockedWaker(_self).wake()
        } else {
            return false
        }
    }

    pub(crate) fn upgrade(&self) -> Option<LockedWaker> {
        if let Some(_self) = self.0.upgrade() {
            if _self.waked.load(Ordering::Acquire) {
                return None;
            }
            return Some(LockedWaker(_self))
        }
        return None
    }
}

#[test]
fn test_waker() {
    println!("waker size {}", std::mem::size_of::<LockedWakerRef>());
}
