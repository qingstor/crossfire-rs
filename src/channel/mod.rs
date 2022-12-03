mod locked_waker;
mod select;
mod stream;
pub use stream::Stream;
pub use select::SelectSame;
pub use locked_waker::*;
use std::task::Context;
use crossbeam::channel::{TryRecvError, RecvError};

/// This defines interface for mpmc channel shared state
pub trait MPMCShared: Sync + Send {
    fn new() -> Self;
    fn on_recv(&self);
    fn on_send(&self);
    fn reg_recv(&self, _ctx: &mut Context) -> Option<LockedWaker>;
    fn reg_send(&self, _ctx: &mut Context) -> Option<LockedWaker>;
    fn add_tx(&self);
    fn add_rx(&self);
    fn close_tx(&self);
    fn close_rx(&self);
    fn get_tx_count(&self) -> usize;

    fn get_waker_length(&self) -> (usize, usize) {
        return (0, 0);
    }

    #[inline]
    fn clear_send_wakers(&self, _waker: LockedWaker) {}

    #[inline]
    fn clear_recv_wakers(&self, _waker: LockedWaker) {}
}

/// This defines interface for mpsc channel shared state
pub trait MPSCShared: Sync + Send {
    fn new() -> Self;
    fn on_recv(&self);
    fn on_send(&self);
    fn cancel_recv_reg(&self);
    fn reg_recv(&self, _ctx: &mut Context) -> Option<LockedWaker>;
    fn reg_send(&self, _ctx: &mut Context) -> Option<LockedWaker>;
    fn add_tx(&self);
    fn close_tx(&self);
    fn close_rx(&self);
    fn get_tx_count(&self) -> usize;

    fn get_waker_length(&self) -> (usize, usize) {
        return (0, 0);
    }

    #[inline]
    fn clear_send_wakers(&self, _waker: LockedWaker) {}
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
    fn poll_item(&self, ctx: &mut Context, waker: &mut Option<LockedWaker>) -> Result<T, TryRecvError>;
    fn clear_recv_wakers(&self, _waker: LockedWaker) {}
    fn on_send(&self) {}
}

macro_rules! clear_sender_wakers_common {
    ($self: expr, $seq: expr) => {
        {
            if $seq & 15 != 0 {
                return;
            }
            let limit = $self.tx_count.load(Ordering::Relaxed) as u64 + 200;
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
        }
    }
}

macro_rules! clear_recv_wakers_common {
    ($self: expr, $seq: expr) => {
        {
            if $seq & 15 != 0 {
                return;
            }
            let limit = $self.rx_count.load(Ordering::Relaxed) as u64 + 500;
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
        }
    }
}

macro_rules! reg_send_m {
    ($self: expr, $ctx: expr) => {
        {
            let seq = $self.send_waker_tx_seq.fetch_add(1, Ordering::SeqCst);
            let waker = LockedWaker::new($ctx, seq);
            let _ = $self.sender_waker.push(waker.weak());
            if $self.rx_count.load(Ordering::SeqCst) == 0 {
                // XXX atomic order?
                waker.cancel();
                return None
            } else {
                return Some(waker)
            }
        }
    }
}

macro_rules! reg_recv_s {
    ($self: expr, $ctx: expr) => {
        {

            let waker = LockedWaker::new($ctx, 0);
            let _ = $self.recv_waker.push(waker.weak());
            if $self.tx_count.load(Ordering::SeqCst) == 0 {
                // no one is sending
                waker.cancel();
                return None
            } else {
                return Some(waker)
            }
        }
    }
}


macro_rules! reg_recv_m {
    ($self: expr, $ctx: expr) => {
        {
            let seq = $self.recv_waker_tx_seq.fetch_add(1, Ordering::SeqCst);
            let waker = LockedWaker::new($ctx, seq);
            let _ = $self.recv_waker.push(waker.weak());
            if $self.tx_count.load(Ordering::SeqCst) == 0 {
                // no one is sending
                waker.cancel();
                return None
            } else {
                return Some(waker)
            }
        }
    }
}

macro_rules! on_recv_m {
    ($self: expr) => {
        {
            loop {
                if let Some(waker) = $self.sender_waker.pop() {
                    let _seq = $self.send_waker_rx_seq.fetch_add(1, Ordering::SeqCst);
                    if waker.wake() {
                        return;
                    }
                } else {
                    return;
                }
            }
        }
    }
}

macro_rules! on_send_m {
    ($self: expr) => {
        {
            loop {
                if let Some(waker) = $self.recv_waker.pop() {
                    let _seq = $self.recv_waker_rx_seq.fetch_add(1, Ordering::SeqCst);
                    if waker.wake() {
                        return;
                    }
                } else {
                    return;
                }
            }
        }
    }
}

macro_rules! on_send_s {
    ($self: expr) => {
        {
            loop {
                if let Some(waker) = $self.recv_waker.pop() {
                    if waker.wake() {
                        return;
                    }
                } else {
                   return;
                }
            }
        }
    }
}

macro_rules! close_tx_common {
    ($self: expr) => {
        {
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
        }
    }
}

macro_rules! close_rx_common {
    ($self: expr) => {
        {
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
        }
    }
}
