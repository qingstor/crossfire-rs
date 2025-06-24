/// Single producer, single consumer
use crate::channel::*;
use crate::rx::*;
use crate::tx::*;

/// Initiate a bounded channel that sender and receiver are async
pub fn bounded_async<T: Unpin>(size: usize) -> (AsyncTx<T>, AsyncRx<T>) {
    let (tx, rx) = crossbeam::channel::bounded(size);
    let send_wakers = SendWakersSingle::new();
    let recv_wakers = RecvWakersSingle::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);

    let tx = AsyncTx::new(tx, shared.clone());
    let rx = AsyncRx::new(rx, shared);
    (tx, rx)
}

/// Initiate a bounded channel that sender is async, receiver is blocking
pub fn bounded_tx_async_rx_blocking<T: Unpin>(size: usize) -> (AsyncTx<T>, Rx<T>) {
    let (tx, rx) = crossbeam::channel::bounded(size);
    let send_wakers = SendWakersSingle::new();
    let recv_wakers = RecvWakersBlocking::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);

    let tx = AsyncTx::new(tx, shared.clone());
    let rx = Rx::new(rx, shared);
    (tx, rx)
}

/// Initiate a bounded channel that sender is blocking, receiver is sync
pub fn bounded_tx_blocking_rx_async<T>(size: usize) -> (Tx<T>, AsyncRx<T>) {
    let (tx, rx) = crossbeam::channel::bounded(size);
    let send_wakers = SendWakersBlocking::new();
    let recv_wakers = RecvWakersSingle::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);

    let tx = Tx::new(tx, shared.clone());
    let rx = AsyncRx::new(rx, shared);
    (tx, rx)
}

/// Initiate a unbounded channel.
/// Sender will never block, so we use the same TxBlocking for threads
pub fn unbounded_async<T: Unpin>() -> (Tx<T>, AsyncRx<T>) {
    let (tx, rx) = crossbeam::channel::unbounded();
    let send_wakers = SendWakersBlocking::new();
    let recv_wakers = RecvWakersSingle::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);
    let tx = Tx::new(tx, shared.clone());
    let rx = AsyncRx::new(rx, shared);
    (tx, rx)
}
