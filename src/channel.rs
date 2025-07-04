pub use super::waker_registry::*;
pub use crate::locked_waker::*;
use crossbeam::queue::{ArrayQueue, SegQueue};
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
    pub fn try_send(&self, item: T) -> Result<(), T> {
        match &self.inner {
            Channel::List(inner) => {
                inner.push(item);
                return Ok(());
            }
            Channel::Array(inner) => {
                return inner.push(item);
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
        self.recvs.fire()
    }

    /// Wake up one tx
    #[inline(always)]
    pub fn on_recv(&self) {
        self.senders.fire()
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
