use crate::locked_waker::*;
use crossbeam::queue::SegQueue;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::task::Context;

#[enum_dispatch(SendWakersTrait)]
pub enum SendWakers {
    Blocking(SendWakersBlocking),
    Single(SendWakersSingle),
    Multi(SendWakersMulti),
}

#[enum_dispatch]
pub trait SendWakersTrait {
    fn reg_send(&self, ctx: &mut Context) -> LockedWaker;

    fn on_recv(&self);

    fn close(&self);

    /// return waker queue size
    fn get_size(&self) -> usize;

    fn clear_send_wakers(&self, _seq: u64) {}

    fn cancel_send_waker(&self, _waker: LockedWaker);
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
    fn clear_send_wakers(&self, _seq: u64) {}

    #[inline(always)]
    fn close(&self) {}

    /// return waker queue size
    #[inline(always)]
    fn get_size(&self) -> usize {
        0
    }

    #[inline(always)]
    fn cancel_send_waker(&self, _waker: LockedWaker) {
        unreachable!();
    }
}

pub struct SendWakersSingle {
    cell: WakerCell,
}

impl SendWakersSingle {
    #[inline(always)]
    pub fn new() -> SendWakers {
        SendWakers::Single(Self { cell: WakerCell::new() })
    }
}

impl SendWakersTrait for SendWakersSingle {
    #[inline(always)]
    fn reg_send(&self, ctx: &mut Context) -> LockedWaker {
        let waker = LockedWaker::new(ctx, 0);
        self.cell.put(&waker);
        waker
    }

    #[inline(always)]
    fn clear_send_wakers(&self, _seq: u64) {
        self.cell.clear();
    }

    #[inline(always)]
    fn cancel_send_waker(&self, _waker: LockedWaker) {
        //        self.cell.clear();
    }

    #[inline(always)]
    fn on_recv(&self) {
        self.cell.wake();
    }

    #[inline]
    fn close(&self) {
        self.on_recv();
    }

    /// return waker queue size
    fn get_size(&self) -> usize {
        0
    }
}

pub struct SendWakersMulti {
    sender_waker: SegQueue<LockedWakerRef>,
    send_waker_tx_seq: AtomicU64,
    checking_sender: AtomicBool,
}

impl SendWakersMulti {
    #[inline(always)]
    pub fn new() -> SendWakers {
        SendWakers::Multi(Self {
            sender_waker: SegQueue::new(),
            send_waker_tx_seq: AtomicU64::new(0),
            checking_sender: AtomicBool::new(false),
        })
    }
}

impl SendWakersTrait for SendWakersMulti {
    #[inline(always)]
    fn reg_send(&self, ctx: &mut Context) -> LockedWaker {
        let seq = self.send_waker_tx_seq.fetch_add(1, Ordering::SeqCst);
        let waker = LockedWaker::new(ctx, seq);
        let _ = self.sender_waker.push(waker.weak());
        waker
    }

    #[inline(always)]
    fn cancel_send_waker(&self, waker: LockedWaker) {
        // Just abandon and leave it to on_recv() to clean it
        waker.abandon();
    }

    /// Call when SendFuture is cancelled.
    /// to clear the LockedWakerRef which has been sent to the other side.
    #[inline(always)]
    fn clear_send_wakers(&self, seq: u64) {
        if self.checking_sender.swap(true, Ordering::SeqCst) {
            // Other thread is cleaning
            return;
        }
        while let Some(waker_ref) = self.sender_waker.pop() {
            if waker_ref.try_to_clear(seq) {
                // we do not known push back may have concurrent problem
                break;
            }
        }
        self.checking_sender.store(false, Ordering::Release);
    }

    #[inline(always)]
    fn on_recv(&self) {
        loop {
            if let Some(waker) = self.sender_waker.pop() {
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
        while let Some(waker) = self.sender_waker.pop() {
            waker.wake();
        }
    }

    /// return waker queue size
    fn get_size(&self) -> usize {
        self.sender_waker.len()
    }
}
