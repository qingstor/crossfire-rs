/// Multi producers, multi consumers
use crate::channel::*;
use crate::m_rx::*;
use crate::m_tx::*;

/// Initiate a unbounded channel.
///
/// Altough sender type is MTx, will never block.
pub fn unbounded_async<T: Unpin>() -> (MTx<T>, MAsyncRx<T>) {
    let (tx, rx) = crossbeam::channel::unbounded();

    let send_wakers = SendWakersBlocking::new();
    let recv_wakers = RecvWakersMulti::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);
    let tx = MTx::new(tx, shared.clone());
    let rx = MAsyncRx::new(rx, shared);
    (tx, rx)
}

/// Initiate a bounded channel that sender and receiver are async.
///
/// Special case: 0 size is not supported yet, threat it as 1 size for now.
pub fn bounded_async<T: Unpin>(mut size: usize) -> (MAsyncTx<T>, MAsyncRx<T>) {
    if size == 0 {
        size = 1;
    }
    let (tx, rx) = crossbeam::channel::bounded(size);
    let send_wakers = SendWakersMulti::new();
    let recv_wakers = RecvWakersMulti::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);
    let tx = MAsyncTx::new(tx, shared.clone());
    let rx = MAsyncRx::new(rx, shared);
    (tx, rx)
}

/// Initiate a bounded channel that sender is async, receiver is blocking.
///
/// Special case: 0 size is not supported yet, threat it as 1 size for now.
pub fn bounded_tx_async_rx_blocking<T: Unpin>(mut size: usize) -> (MAsyncTx<T>, MRx<T>) {
    if size == 0 {
        size = 1;
    }
    let (tx, rx) = crossbeam::channel::bounded(size);
    let send_wakers = SendWakersMulti::new();
    let recv_wakers = RecvWakersBlocking::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);

    let tx = MAsyncTx::new(tx, shared.clone());
    let rx = MRx::new(rx, shared);
    (tx, rx)
}

/// Initiate a bounded channel that sender is blocking, receiver is async
///
/// Special case: 0 size is not supported yet, threat it as 1 size for now.
pub fn bounded_tx_blocking_rx_async<T: Unpin>(mut size: usize) -> (MTx<T>, MAsyncRx<T>) {
    if size == 0 {
        size = 1;
    }
    let (tx, rx) = crossbeam::channel::bounded(size);
    let send_wakers = SendWakersBlocking::new();
    let recv_wakers = RecvWakersMulti::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);

    let tx = MTx::new(tx, shared.clone());
    let rx = MAsyncRx::new(rx, shared);
    (tx, rx)
}
