use crate::async_rx::*;
use crate::async_tx::*;
use crate::blocking_rx::*;
use crate::blocking_tx::*;
/// Single producer, single consumer
use crate::channel::*;

/// Initiate an unbounded channel for blocking context.
///
/// Sender will never block, so we use the same TxBlocking for threads
pub fn unbounded_blocking<T: Unpin>() -> (Tx<T>, Rx<T>) {
    let (tx, rx) = crossbeam::channel::unbounded();
    let send_wakers = SendWakersBlocking::new();
    let recv_wakers = RecvWakersBlocking::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);
    let tx = Tx::new(tx, shared.clone());
    let rx = Rx::new(rx, shared);
    (tx, rx)
}

/// Initiate an unbounded channel for async context.
///
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

/// Initiate a bounded channel for blocking context
///
/// Special case: 0 size is not supported yet, threat it as 1 size for now.
pub fn bounded_blocking<T: Unpin>(mut size: usize) -> (Tx<T>, Rx<T>) {
    if size == 0 {
        size = 1;
    }
    let (tx, rx) = crossbeam::channel::bounded(size);
    let send_wakers = SendWakersBlocking::new();
    let recv_wakers = RecvWakersBlocking::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);

    let tx = Tx::new(tx, shared.clone());
    let rx = Rx::new(rx, shared);
    (tx, rx)
}

/// Initiate a bounded channel that sender and receiver are async
///
/// Special case: 0 size is not supported yet, threat it as 1 size for now.
pub fn bounded_async<T: Unpin>(mut size: usize) -> (AsyncTx<T>, AsyncRx<T>) {
    if size == 0 {
        size = 1;
    }
    let (tx, rx) = crossbeam::channel::bounded(size);
    let send_wakers = SendWakersSingle::new();
    let recv_wakers = RecvWakersSingle::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);

    let tx = AsyncTx::new(tx, shared.clone());
    let rx = AsyncRx::new(rx, shared);
    (tx, rx)
}

/// Initiate a bounded channel that sender is async, receiver is blocking
///
/// Special case: 0 size is not supported yet, threat it as 1 size for now.
pub fn bounded_tx_async_rx_blocking<T: Unpin>(mut size: usize) -> (AsyncTx<T>, Rx<T>) {
    if size == 0 {
        size = 1;
    }
    let (tx, rx) = crossbeam::channel::bounded(size);
    let send_wakers = SendWakersSingle::new();
    let recv_wakers = RecvWakersBlocking::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);

    let tx = AsyncTx::new(tx, shared.clone());
    let rx = Rx::new(rx, shared);
    (tx, rx)
}

/// Initiate a bounded channel that sender is blocking, receiver is sync
///
/// Special case: 0 size is not supported yet, threat it as 1 size for now.
pub fn bounded_tx_blocking_rx_async<T>(mut size: usize) -> (Tx<T>, AsyncRx<T>) {
    if size == 0 {
        size = 1;
    }
    let (tx, rx) = crossbeam::channel::bounded(size);
    let send_wakers = SendWakersBlocking::new();
    let recv_wakers = RecvWakersSingle::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);

    let tx = Tx::new(tx, shared.clone());
    let rx = AsyncRx::new(rx, shared);
    (tx, rx)
}
