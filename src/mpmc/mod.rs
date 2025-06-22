mod tx;
pub use tx::*;
mod rx;
pub use rx::*;
mod bounded;
pub use bounded::{SharedFutureBoth, SharedSenderBRecvF, SharedSenderFRecvB};
pub use bounded::{
    bounded_future_both, bounded_tx_blocking_rx_future, bounded_tx_future_rx_blocking,
};
mod unbounded;
pub use unbounded::{RxUnbounded, TxUnbounded, UnboundedSharedFuture, unbounded_future};

macro_rules! reg_send_m {
    ($self: expr, $ctx: expr) => {{
        let seq = $self.send_waker_tx_seq.fetch_add(1, Ordering::SeqCst);
        let waker = LockedWaker::new($ctx, seq);
        let _ = $self.sender_waker.push(waker.weak());
        if $self.rx_count.load(Ordering::SeqCst) == 0 {
            // XXX atomic order?
            waker.cancel();
            return None;
        } else {
            return Some(waker);
        }
    }};
}
pub(super) use reg_send_m;

macro_rules! on_send_m {
    ($self: expr) => {{
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
    }};
}
pub(super) use on_send_m;

macro_rules! reg_recv_m {
    ($self: expr, $ctx: expr) => {{
        let seq = $self.recv_waker_tx_seq.fetch_add(1, Ordering::SeqCst);
        let waker = LockedWaker::new($ctx, seq);
        let _ = $self.recv_waker.push(waker.weak());
        if $self.tx_count.load(Ordering::SeqCst) == 0 {
            // no one is sending
            waker.cancel();
            return None;
        } else {
            return Some(waker);
        }
    }};
}
pub(super) use reg_recv_m;

macro_rules! on_recv_m {
    ($self: expr) => {{
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
    }};
}
pub(super) use on_recv_m;
