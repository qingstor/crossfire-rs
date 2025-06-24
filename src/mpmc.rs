/// Multi producers, multi consumers
use crate::channel::*;
use crate::m_rx::*;
use crate::m_tx::*;

/// Initiate a unbounded channel.
/// Sender will never block, so we use the same TxBlocking for threads
pub fn unbounded_async<T: Unpin>() -> (MTx<T>, MAsyncRx<T>) {
    let (tx, rx) = crossbeam::channel::unbounded();

    let send_wakers = SendWakersBlocking::new();
    let recv_wakers = RecvWakersMulti::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);
    let tx = MTx::new(tx, shared.clone());
    let rx = MAsyncRx::new(rx, shared);
    (tx, rx)
}

/// Initiate a bounded channel that sender and receiver are async
pub fn bounded_async<T: Unpin>(size: usize) -> (MAsyncTx<T>, MAsyncRx<T>) {
    let (tx, rx) = crossbeam::channel::bounded(size);
    let send_wakers = SendWakersMulti::new();
    let recv_wakers = RecvWakersMulti::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);
    let tx = MAsyncTx::new(tx, shared.clone());
    let rx = MAsyncRx::new(rx, shared);
    (tx, rx)
}

/// Initiate a bounded channel that sender is async, receiver is blocking
pub fn bounded_tx_async_rx_blocking<T: Unpin>(size: usize) -> (MAsyncTx<T>, MRx<T>) {
    let (tx, rx) = crossbeam::channel::bounded(size);
    let send_wakers = SendWakersMulti::new();
    let recv_wakers = RecvWakersBlocking::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);

    let tx = MAsyncTx::new(tx, shared.clone());
    let rx = MRx::new(rx, shared);
    (tx, rx)
}

/// Initiate a bounded channel that sender is blocking, receiver is sync
pub fn bounded_tx_blocking_rx_async<T: Unpin>(size: usize) -> (MTx<T>, MAsyncRx<T>) {
    let (tx, rx) = crossbeam::channel::bounded(size);
    let send_wakers = SendWakersBlocking::new();
    let recv_wakers = RecvWakersMulti::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);

    let tx = MTx::new(tx, shared.clone());
    let rx = MAsyncRx::new(rx, shared);
    (tx, rx)
}
