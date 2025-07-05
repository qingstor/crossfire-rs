pub use super::waker_registry::*;
pub use crate::locked_waker::*;
use crossbeam::queue::{ArrayQueue, SegQueue};
use lazy_static::lazy_static;
use std::mem;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::Context;

pub enum Channel<T> {
    List(SegQueue<T>),
    Array(ArrayQueue<T>),
}

impl<T> Channel<T> {
    #[inline(always)]
    pub fn new_list() -> Self {
        Self::List(SegQueue::new())
    }

    #[inline(always)]
    pub fn new_array(bound: usize) -> Self {
        Self::Array(ArrayQueue::new(bound))
    }

    #[inline(always)]
    pub fn get_bound(&self) -> usize {
        match self {
            Self::List(_) => 0,
            Self::Array(s) => s.capacity(),
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        match self {
            Self::List(s) => s.len(),
            Self::Array(s) => s.len(),
        }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::List(s) => s.is_empty(),
            Self::Array(s) => s.is_empty(),
        }
    }
}

pub struct ChannelShared<T> {
    recvs: Registry,
    senders: Registry,
    tx_count: AtomicU64,
    rx_count: AtomicU64,
    inner: Channel<T>,
    pub bound_size: usize,
}

impl<T: Send + 'static> ChannelShared<T> {
    pub fn try_send(&self, item: &mem::MaybeUninit<T>) -> Result<(), ()> {
        match &self.inner {
            Channel::List(inner) => {
                inner.push(unsafe { item.assume_init_read() });
                return Ok(());
            }
            Channel::Array(inner) => {
                if let Err(t) = inner.push(unsafe { item.assume_init_read() }) {
                    mem::forget(t);
                    return Err(());
                } else {
                    return Ok(());
                }
            }
        }
    }
}

impl<T> ChannelShared<T> {
    pub fn new(inner: Channel<T>, senders: Registry, recvs: Registry) -> Arc<Self> {
        Arc::new(Self {
            tx_count: AtomicU64::new(1),
            rx_count: AtomicU64::new(1),
            senders,
            recvs,
            bound_size: inner.get_bound(),
            inner,
        })
    }

    #[inline(always)]
    pub fn try_recv(&self) -> Option<T> {
        match &self.inner {
            Channel::List(inner) => {
                return inner.pop();
            }
            Channel::Array(inner) => {
                return inner.pop();
            }
        }
    }

    #[inline(always)]
    pub fn get_tx_count(&self) -> usize {
        self.tx_count.load(Ordering::Acquire) as usize
    }

    #[inline(always)]
    pub fn get_rx_count(&self) -> usize {
        self.rx_count.load(Ordering::Acquire) as usize
    }

    #[inline(always)]
    pub fn add_tx(&self) {
        let _ = self.tx_count.fetch_add(1, Ordering::SeqCst);
    }

    #[inline(always)]
    pub fn add_rx(&self) {
        let _ = self.rx_count.fetch_add(1, Ordering::SeqCst);
    }

    /// Call when tx drop
    #[inline(always)]
    pub fn close_tx(&self) {
        if self.tx_count.fetch_sub(1, Ordering::SeqCst) <= 1 {
            self.recvs.close();
        }
    }

    /// Call when rx drop
    #[inline(always)]
    pub fn close_rx(&self) {
        if self.rx_count.fetch_sub(1, Ordering::SeqCst) <= 1 {
            self.senders.close();
        }
    }

    /// Register waker for current rx
    #[inline(always)]
    pub fn reg_recv_async(&self, ctx: &mut Context, o_waker: &mut Option<LockedWaker>) -> bool {
        self.recvs.reg_async(ctx, o_waker)
    }

    /// Register waker for current tx
    #[inline(always)]
    pub fn reg_send_async(&self, ctx: &mut Context, o_waker: &mut Option<LockedWaker>) -> bool {
        self.senders.reg_async(ctx, o_waker)
    }

    #[inline(always)]
    pub fn reg_send_blocking(&self, waker: &LockedWaker) {
        self.senders.reg_blocking(waker);
    }

    #[inline(always)]
    pub fn reg_recv_blocking(&self, waker: &LockedWaker) {
        self.recvs.reg_blocking(waker);
    }

    /// Wake up one rx
    #[inline(always)]
    pub fn on_send(&self) {
        self.recvs.fire();
    }

    /// Wake up one tx
    #[inline(always)]
    pub fn on_recv(&self) {
        self.senders.fire();
    }

    /// On timeout, clear dead wakers on sender queue
    pub fn clear_send_wakers(&self, seq: u64) {
        self.senders.clear_wakers(seq);
    }

    /// On timeout, clear dead wakers on receiver queue
    #[inline(always)]
    pub fn clear_recv_wakers(&self, seq: u64) {
        self.recvs.clear_wakers(seq);
    }

    /// Just for debugging purpose, to monitor queue size
    #[cfg(test)]
    pub fn get_waker_size(&self) -> (usize, usize) {
        (self.senders.get_size(), self.recvs.get_size())
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

#[allow(dead_code)]
#[derive(Default)]
pub struct ChannelStats {
    tx_try: AtomicU64,
    tx_poll: AtomicU64,
    tx_done: AtomicU64,
    rx_try: AtomicU64,
    rx_poll: AtomicU64,
    rx_done: AtomicU64,
}

lazy_static! {
    static ref STATS: ChannelStats = Default::default();
}

#[cfg(feature = "profile")]
impl ChannelStats {
    pub fn to_string() -> String {
        let mut tx_try = STATS.tx_try.load(Ordering::Acquire) as f64;
        let mut tx_poll = STATS.tx_poll.load(Ordering::Acquire) as f64;
        let tx_done = STATS.tx_done.load(Ordering::Acquire);
        let mut rx_try = STATS.rx_try.load(Ordering::Acquire) as f64;
        let mut rx_poll = STATS.rx_poll.load(Ordering::Acquire) as f64;
        let rx_done = STATS.rx_done.load(Ordering::Acquire);
        if tx_done > 0 {
            tx_try /= tx_poll;
            tx_poll /= tx_done as f64;
        }
        if rx_done > 0 {
            rx_try /= rx_poll;
            rx_poll /= rx_done as f64;
        }
        format!(
            "tx:[avg(try={}, poll={}) op={}], rx[avg(try={}, poll={}), op={}]",
            tx_try, tx_poll, tx_done, rx_try, rx_poll, rx_done,
        )
        .to_string()
    }

    pub fn clear() {
        STATS.tx_try.store(0, Ordering::Release);
        STATS.rx_try.store(0, Ordering::Release);
        STATS.tx_poll.store(0, Ordering::Release);
        STATS.rx_poll.store(0, Ordering::Release);
        STATS.tx_done.store(0, Ordering::Release);
        STATS.rx_done.store(0, Ordering::Release);
    }

    #[inline(always)]
    pub(crate) fn tx_poll(retry: usize) {
        STATS.tx_try.fetch_add(retry as u64, Ordering::SeqCst);
        STATS.tx_poll.fetch_add(1, Ordering::SeqCst);
    }

    #[inline(always)]
    pub(crate) fn rx_poll(retry: usize) {
        STATS.rx_try.fetch_add(retry as u64, Ordering::SeqCst);
        STATS.rx_poll.fetch_add(1, Ordering::SeqCst);
    }

    #[inline(always)]
    pub(crate) fn tx_done() {
        STATS.tx_done.fetch_add(1, Ordering::SeqCst);
    }

    #[inline(always)]
    pub(crate) fn rx_done() {
        STATS.rx_done.fetch_add(1, Ordering::SeqCst);
    }
}
