pub use crate::locked_waker::*;
use crossbeam::channel::{RecvError, TryRecvError};
use std::task::Context;

/// This defines interface for mpmc channel shared state
pub trait ChannelShared: Sync + Send {
    fn new() -> Self;
    fn on_recv(&self);
    fn on_send(&self);
    fn reg_recv(&self, _ctx: &mut Context) -> Option<LockedWaker>;
    fn reg_send(&self, _ctx: &mut Context) -> Option<LockedWaker>;
    fn add_tx(&self);
    #[inline(always)]
    fn add_rx(&self) {}
    fn close_tx(&self);
    fn close_rx(&self);
    fn get_tx_count(&self) -> usize;

    #[inline(always)]
    fn get_waker_length(&self) -> (usize, usize) {
        return (0, 0);
    }

    /// seq: The seq of waker
    #[inline(always)]
    fn clear_send_wakers(&self, _seq: u64) {}
    #[inline(always)]
    fn clear_recv_wakers(&self, _seq: u64) {}
}

/// This defines interface of async receivers (for SelectSame)
#[async_trait]
pub trait AsyncRx<T>: Sync + Send
where
    T: Send + Sync + 'static,
{
    async fn recv(&self) -> Result<T, RecvError>;
    fn try_recv(&self) -> Result<T, TryRecvError>;
    fn is_empty(&self) -> bool;
    fn poll_item(
        &self, ctx: &mut Context, waker: &mut Option<LockedWaker>,
    ) -> Result<T, TryRecvError>;
    #[inline(always)]
    fn clear_recv_wakers(&self, _seq: u64) {}
    fn on_send(&self) {}
}

macro_rules! clear_sender_wakers_common {
    ($self: expr, $seq: expr) => {{
        if $seq & 15 != 0 {
            return;
        }
        let limit = $self.tx_count.load(Ordering::Acquire) as u64 + 200;
        if $self.send_waker_rx_seq.load(Ordering::Acquire) + limit >= $seq {
            return;
        }
        if !$self.checking_sender.swap(true, Ordering::SeqCst) {
            let mut ok = true;
            while ok {
                if let Some(waker) = $self.sender_waker.pop() {
                    ok = $self.send_waker_rx_seq.fetch_add(1, Ordering::SeqCst) + limit < $seq;
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
            $self.checking_sender.store(false, Ordering::Release);
        }
    }};
}
pub(super) use clear_sender_wakers_common;

macro_rules! clear_recv_wakers_common {
    ($self: expr, $seq: expr) => {{
        if $seq & 15 != 0 {
            return;
        }
        let limit = $self.rx_count.load(Ordering::Acquire) as u64 + 500;
        if $self.recv_waker_rx_seq.load(Ordering::Acquire) + limit >= $seq {
            return;
        }
        if !$self.checking_recv.swap(true, Ordering::SeqCst) {
            let mut ok = true;
            while ok {
                if let Some(waker) = $self.recv_waker.pop() {
                    ok = $self.recv_waker_rx_seq.fetch_add(1, Ordering::SeqCst) + limit < $seq;
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
            $self.checking_recv.store(false, Ordering::Release);
        }
    }};
}
pub(super) use clear_recv_wakers_common;

macro_rules! close_tx_common {
    ($self: expr) => {{
        if $self.tx_count.fetch_sub(1, Ordering::SeqCst) > 1 {
            return;
        }
        // wake all rx, since no one will wake blocked future after that
        loop {
            if let Some(waker) = $self.recv_waker.pop() {
                waker.wake();
            } else {
                return;
            }
        }
    }};
}
pub(super) use close_tx_common;

macro_rules! close_rx_common {
    ($self: expr) => {{
        if $self.rx_count.fetch_sub(1, Ordering::SeqCst) > 1 {
            return;
        }
        // wake all tx, since no one will wake blocked future after that
        loop {
            if let Some(waker) = $self.sender_waker.pop() {
                waker.wake();
            } else {
                return;
            }
        }
    }};
}
pub(super) use close_rx_common;
