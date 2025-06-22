use crate::locked_waker::*;
use crossbeam::queue::SegQueue;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::task::Context;

#[enum_dispatch(SendWakersTrait)]
pub enum SendWakers {
    Blocking(SendWakersBlocking),
    Multi(SendWakersMulti),
}

#[enum_dispatch]
pub trait SendWakersTrait {
    fn reg_send(&self, ctx: &mut Context) -> LockedWaker;

    fn on_recv(&self);

    fn close(&self);

    /// return waker queue size
    fn get_size(&self) -> usize;

    fn clear_send_wakers(&self, _seq: u64, _limit: u64) {}
}

pub struct SendWakersBlocking {}

impl SendWakersBlocking {
    #[inline(always)]
    pub fn new() -> SendWakers {
        SendWakers::Blocking(Self {})
    }
}

impl SendWakersTrait for SendWakersBlocking {
    #[inline]
    fn reg_send(&self, _ctx: &mut Context) -> LockedWaker {
        unreachable!();
    }

    #[inline(always)]
    fn on_recv(&self) {}

    #[inline(always)]
    fn clear_send_wakers(&self, _seq: u64, _limit: u64) {}

    #[inline(always)]
    fn close(&self) {}

    /// return waker queue size
    fn get_size(&self) -> usize {
        0
    }
}

pub struct SendWakersMulti {
    sender_waker: SegQueue<LockedWakerRef>,
    send_waker_tx_seq: AtomicU64,
    send_waker_rx_seq: AtomicU64,
    checking_sender: AtomicBool,
}

impl SendWakersMulti {
    #[inline(always)]
    pub fn new() -> SendWakers {
        SendWakers::Multi(Self {
            sender_waker: SegQueue::new(),
            send_waker_tx_seq: AtomicU64::new(0),
            send_waker_rx_seq: AtomicU64::new(0),
            checking_sender: AtomicBool::new(false),
        })
    }
}

impl SendWakersTrait for SendWakersMulti {
    #[inline]
    fn reg_send(&self, ctx: &mut Context) -> LockedWaker {
        let seq = self.send_waker_tx_seq.fetch_add(1, Ordering::SeqCst);
        let waker = LockedWaker::new(ctx, seq);
        let _ = self.sender_waker.push(waker.weak());
        waker
    }

    #[inline]
    fn clear_send_wakers(&self, seq: u64, limit: u64) {
        if seq & 15 != 0 {
            return;
        }
        if self.send_waker_rx_seq.load(Ordering::Acquire) + limit >= seq {
            return;
        }
        if !self.checking_sender.swap(true, Ordering::SeqCst) {
            let mut ok = true;
            while ok {
                if let Some(waker) = self.sender_waker.pop() {
                    ok = self.send_waker_rx_seq.fetch_add(1, Ordering::SeqCst) + limit < seq;
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
            self.checking_sender.store(false, Ordering::Release);
        }
    }

    #[inline]
    fn on_recv(&self) {
        loop {
            if let Some(waker) = self.sender_waker.pop() {
                let _seq = self.send_waker_rx_seq.fetch_add(1, Ordering::SeqCst);
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
        // wake all tx, since no one will wake blocked future after that
        loop {
            if let Some(waker) = self.sender_waker.pop() {
                waker.wake();
            } else {
                return;
            }
        }
    }

    /// return waker queue size
    fn get_size(&self) -> usize {
        self.sender_waker.len()
    }
}
