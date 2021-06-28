use std::sync::{Arc, Weak, atomic::{AtomicBool, Ordering, spin_loop_hint}};
use std::task::{Context, Waker};
use std::cell::UnsafeCell;
use std::mem::transmute;

pub struct LockedWaker (Arc<LockedWakerInner>);

struct LockedWakerInner {
    locked: AtomicBool,
    waker: UnsafeCell<Option<std::task::Waker>>,
    waked: AtomicBool,
    seq: u64,
}

unsafe impl Send for LockedWakerInner {}
unsafe impl Sync for LockedWakerInner {}

impl Clone for LockedWaker {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

pub struct LockedWakerRef (Weak<LockedWakerInner>);

impl LockedWaker {

    #[inline(always)]
    pub(crate) fn new(locked: bool, seq: u64) -> Self {
        let s = Arc::new(LockedWakerInner{
            seq: seq,
            locked: AtomicBool::new(locked), // initial locked
            waker: UnsafeCell::new(None),
            waked: AtomicBool::new(false),
        });
        Self(s)
    }

    // return again
    #[inline(always)]
    pub(crate) fn commit(&self, ctx: &mut Context) -> bool {
        let _self = self.0.as_ref();
        let waker: &mut Option<Waker> = unsafe{transmute(_self.waker.get())};
        waker.replace(ctx.waker().clone());
        _self.locked.store(false, Ordering::Release);
        return false
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
            spin_loop_hint();
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
            spin_loop_hint();
        }
        if _self.waked.swap(true, Ordering::SeqCst) == false {
            let waker: &mut Option<Waker> = unsafe{transmute(_self.waker.get())};
            if let Some(_waker) = waker.take() {
                _waker.wake();
                _self.locked.store(false, Ordering::Release);
                return true;
            }
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
