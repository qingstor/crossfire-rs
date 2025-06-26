pub use super::recv_wakers::*;
pub use super::send_wakers::*;
pub use crate::locked_waker::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::Context;

pub struct ChannelShared {
    tx_count: AtomicU64,
    rx_count: AtomicU64,
    recvs: RecvWakers,
    senders: SendWakers,
}

impl ChannelShared {
    pub fn new(senders: SendWakers, recvs: RecvWakers) -> Arc<Self> {
        Arc::new(Self { tx_count: AtomicU64::new(1), rx_count: AtomicU64::new(1), senders, recvs })
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
    pub fn reg_recv(&self, ctx: &mut Context) -> LockedWaker {
        self.recvs.reg_recv(ctx)
    }

    /// Clear dead wakers on rx queue
    #[inline(always)]
    pub fn clear_recv_wakers(&self, seq: u64) {
        self.recvs.clear_recv_wakers(seq);
    }

    /// Wake up one rx
    #[inline(always)]
    pub fn on_send(&self) {
        self.recvs.on_send()
    }

    /// Register waker for current tx
    #[inline(always)]
    pub fn reg_send(&self, ctx: &mut Context) -> LockedWaker {
        self.senders.reg_send(ctx)
    }

    /// Wake up one tx
    #[inline(always)]
    pub fn on_recv(&self) {
        self.senders.on_recv()
    }

    /// Clear dead wakers on sender queue
    pub fn clear_send_wakers(&self, seq: u64) {
        self.senders.clear_send_wakers(seq);
    }

    /// Just for debugging purpose, to monitor queue size
    #[cfg(test)]
    pub fn get_waker_size(&self) -> (usize, usize) {
        (self.senders.get_size(), self.recvs.get_size())
    }
}
