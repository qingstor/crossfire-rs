use crate::locked_waker::*;
use crossbeam::queue::{ArrayQueue, SegQueue};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::task::Context;

#[enum_dispatch(RecvWakersTrait)]
pub enum RecvWakers {
    Blocking(RecvWakersBlocking),
    Single(RecvWakersSingle),
    Multi(RecvWakersMulti),
}

#[enum_dispatch]
pub trait RecvWakersTrait {
    fn reg_recv(&self, ctx: &mut Context) -> LockedWaker;

    fn clear_recv_wakers(&self, _seq: u64, _limit: u64);

    fn on_send(&self);

    fn close(&self);

    /// return waker queue size
    fn get_size(&self) -> usize;
}

pub struct RecvWakersBlocking();

impl RecvWakersBlocking {
    #[inline(always)]
    pub fn new() -> RecvWakers {
        RecvWakers::Blocking(Self())
    }
}

impl RecvWakersTrait for RecvWakersBlocking {
    #[inline(always)]
    fn reg_recv(&self, _ctx: &mut Context) -> LockedWaker {
        unreachable!();
    }

    #[inline(always)]
    fn clear_recv_wakers(&self, _seq: u64, _limit: u64) {}

    #[inline(always)]
    fn on_send(&self) {}

    #[inline(always)]
    fn close(&self) {}

    /// return waker queue size
    fn get_size(&self) -> usize {
        0
    }
}

pub struct RecvWakersSingle {
    recv_waker: ArrayQueue<LockedWakerRef>,
}

impl RecvWakersSingle {
    #[inline(always)]
    pub fn new() -> RecvWakers {
        RecvWakers::Single(Self { recv_waker: ArrayQueue::new(1) })
    }
}

impl RecvWakersTrait for RecvWakersSingle {
    #[inline(always)]
    fn reg_recv(&self, ctx: &mut Context) -> LockedWaker {
        let waker = LockedWaker::new(ctx, 0);
        let weak = waker.weak();
        match self.recv_waker.push(weak) {
            Ok(_) => {}
            Err(_weak) => {
                let _old_waker = self.recv_waker.pop();
                self.recv_waker.push(_weak).expect("push wake ok after pop");
            }
        }
        waker
    }

    #[inline(always)]
    fn clear_recv_wakers(&self, _seq: u64, _limit: u64) {}

    #[inline]
    fn on_send(&self) {
        if let Some(waker) = self.recv_waker.pop() {
            waker.wake();
        }
    }

    #[inline]
    fn close(&self) {
        self.on_send();
    }

    /// return waker queue size
    fn get_size(&self) -> usize {
        self.recv_waker.len()
    }
}

pub struct RecvWakersMulti {
    recv_waker: SegQueue<LockedWakerRef>,
    recv_waker_tx_seq: AtomicU64,
    recv_waker_rx_seq: AtomicU64,
    checking_recv: AtomicBool,
}

impl RecvWakersMulti {
    #[inline(always)]
    pub fn new() -> RecvWakers {
        RecvWakers::Multi(Self {
            recv_waker: SegQueue::new(),
            recv_waker_tx_seq: AtomicU64::new(0),
            recv_waker_rx_seq: AtomicU64::new(0),
            checking_recv: AtomicBool::new(false),
        })
    }
}

impl RecvWakersTrait for RecvWakersMulti {
    #[inline(always)]
    fn reg_recv(&self, ctx: &mut Context) -> LockedWaker {
        let seq = self.recv_waker_tx_seq.fetch_add(1, Ordering::SeqCst);
        let waker = LockedWaker::new(ctx, seq);
        self.recv_waker.push(waker.weak());
        waker
    }

    #[inline]
    fn clear_recv_wakers(&self, seq: u64, limit: u64) {
        if seq & 15 != 0 {
            return;
        }
        if self.recv_waker_rx_seq.load(Ordering::Acquire) + limit >= seq {
            return;
        }
        if !self.checking_recv.swap(true, Ordering::SeqCst) {
            let mut ok = true;
            while ok {
                if let Some(waker) = self.recv_waker.pop() {
                    ok = self.recv_waker_rx_seq.fetch_add(1, Ordering::SeqCst) + limit < seq;
                    if let Some(real_waker) = waker.upgrade() {
                        if !real_waker.is_canceled() {
                            if real_waker.wake() {
                                // we do not known push back may have concurrent problem
                                break;
                            }
                        }
                    }
                } else {
                    break;
                }
            }
            self.checking_recv.store(false, Ordering::Release);
        }
    }

    #[inline]
    fn on_send(&self) {
        loop {
            if let Some(waker) = self.recv_waker.pop() {
                let _seq = self.recv_waker_rx_seq.fetch_add(1, Ordering::SeqCst);
                if waker.wake() {
                    return;
                }
            } else {
                return;
            }
        }
    }

    #[inline]
    fn close(&self) {
        loop {
            if let Some(waker) = self.recv_waker.pop() {
                waker.wake();
            } else {
                return;
            }
        }
    }

    /// return waker queue size
    fn get_size(&self) -> usize {
        self.recv_waker.len()
    }
}
