use crate::locked_waker::*;
use crossbeam::queue::SegQueue;
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

    fn clear_recv_wakers(&self, _seq: u64);

    fn cancel_recv_waker(&self, _waker: LockedWaker);

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
    fn cancel_recv_waker(&self, _waker: LockedWaker) {
        unreachable!();
    }

    #[inline(always)]
    fn clear_recv_wakers(&self, _seq: u64) {}

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
    cell: WakerCell,
}

impl RecvWakersSingle {
    #[inline(always)]
    pub fn new() -> RecvWakers {
        RecvWakers::Single(Self { cell: WakerCell::new() })
    }
}

impl RecvWakersTrait for RecvWakersSingle {
    #[inline(always)]
    fn reg_recv(&self, ctx: &mut Context) -> LockedWaker {
        let waker = LockedWaker::new(ctx, 0);
        self.cell.put(&waker);
        waker
    }

    #[inline(always)]
    fn cancel_recv_waker(&self, _waker: LockedWaker) {
        //        self.cell.clear();
    }

    #[inline(always)]
    fn clear_recv_wakers(&self, _seq: u64) {
        self.cell.clear();
    }

    #[inline(always)]
    fn on_send(&self) {
        self.cell.wake();
    }

    #[inline(always)]
    fn close(&self) {
        self.on_send();
    }

    /// return waker queue size
    fn get_size(&self) -> usize {
        0
    }
}

pub struct RecvWakersMulti {
    recv_waker: SegQueue<LockedWakerRef>,
    recv_waker_tx_seq: AtomicU64,
    checking_recv: AtomicBool,
}

impl RecvWakersMulti {
    #[inline(always)]
    pub fn new() -> RecvWakers {
        RecvWakers::Multi(Self {
            recv_waker: SegQueue::new(),
            recv_waker_tx_seq: AtomicU64::new(0),
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

    #[inline(always)]
    fn cancel_recv_waker(&self, waker: LockedWaker) {
        // Just abandon and leave it to on_send() to clean it
        waker.abandon();
    }

    /// Call when ReceiveFuture is cancelled.
    /// to clear the LockedWakerRef which has been sent to the other side.
    #[inline(always)]
    fn clear_recv_wakers(&self, seq: u64) {
        if self.checking_recv.swap(true, Ordering::SeqCst) {
            // Other thread is cleaning
            return;
        }
        while let Some(waker_ref) = self.recv_waker.pop() {
            if waker_ref.try_to_clear(seq) {
                // we do not known push back may have concurrent problem
                break;
            }
        }
        self.checking_recv.store(false, Ordering::Release);
    }

    #[inline(always)]
    fn on_send(&self) {
        while let Some(waker) = self.recv_waker.pop() {
            if waker.wake() {
                return;
            }
        }
    }

    #[inline]
    fn close(&self) {
        while let Some(waker) = self.recv_waker.pop() {
            waker.wake();
        }
    }

    /// return waker queue size
    fn get_size(&self) -> usize {
        self.recv_waker.len()
    }
}
