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

macro_rules! on_send_s {
    ($self: expr) => {{
        loop {
            if let Some(waker) = $self.recv_waker.pop() {
                if waker.wake() {
                    return;
                }
            } else {
                return;
            }
        }
    }};
}
pub(super) use on_send_s;

macro_rules! reg_recv_s {
    ($self: expr, $ctx: expr) => {{
        let waker = LockedWaker::new($ctx, 0);
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
pub(super) use reg_recv_s;
