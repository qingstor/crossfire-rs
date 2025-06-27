/// Multi producers, single consumer
use crate::channel::*;
use crate::m_tx::*;
use crate::rx::*;

/// Initiate a unbounded channel.
///
/// Although sender type is MTx, will never block.
pub fn unbounded_async<T: Unpin>() -> (MTx<T>, AsyncRx<T>) {
    let (tx, rx) = crossbeam::channel::unbounded();
    let send_wakers = SendWakersBlocking::new();
    let recv_wakers = RecvWakersSingle::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);
    let tx = MTx::new(tx, shared.clone());
    let rx = AsyncRx::new(rx, shared);
    (tx, rx)
}

/// Initiate a bounded channel that sender and receiver is async.
///
/// Special case: 0 size is not supported yet, threat it as 1 size for now.
pub fn bounded_async<T: Unpin>(mut size: usize) -> (MAsyncTx<T>, AsyncRx<T>) {
    if size == 0 {
        size = 1;
    }
    let (tx, rx) = crossbeam::channel::bounded(size);
    let send_wakers = SendWakersMulti::new();
    let recv_wakers = RecvWakersSingle::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);

    let tx = MAsyncTx::new(tx, shared.clone());
    let rx = AsyncRx::new(rx, shared);
    (tx, rx)
}

/// Initiate a bounded channel that sender is async, receiver is blocking.
///
/// Special case: 0 size is not supported yet, threat it as 1 size for now.
pub fn bounded_tx_async_rx_blocking<T: Unpin>(mut size: usize) -> (MAsyncTx<T>, Rx<T>) {
    if size == 0 {
        size = 1;
    }
    let (tx, rx) = crossbeam::channel::bounded(size);
    let send_wakers = SendWakersMulti::new();
    let recv_wakers = RecvWakersBlocking::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);

    let tx = MAsyncTx::new(tx, shared.clone());
    let rx = Rx::new(rx, shared);
    (tx, rx)
}

/// Initiate a bounded channel that sender is blocking, receiver is async.
///
/// Special case: 0 size is not supported yet, threat it as 1 size for now.
pub fn bounded_tx_blocking_rx_async<T>(mut size: usize) -> (MTx<T>, AsyncRx<T>) {
    if size == 0 {
        size = 1;
    }
    let (tx, rx) = crossbeam::channel::bounded(size);
    let send_wakers = SendWakersBlocking::new();
    let recv_wakers = RecvWakersSingle::new();
    let shared = ChannelShared::new(send_wakers, recv_wakers);

    let tx = MTx::new(tx, shared.clone());
    let rx = AsyncRx::new(rx, shared);
    (tx, rx)
}
