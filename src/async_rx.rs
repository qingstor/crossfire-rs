use crate::blocking_rx::Rx;
use crate::channel::*;
use crate::stream::AsyncStream;
use async_trait::async_trait;
pub use crossbeam::channel::{RecvError, TryRecvError};
use crossbeam::utils::Backoff;
use std::fmt;
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

/// Receiver that works in async context
///
/// **NOTE: this is not cloneable.**
/// If you need concurrent access, use [MAsyncRx](crate::MAsyncRx) instead.
pub struct AsyncRx<T> {
    pub(crate) shared: Arc<ChannelShared<T>>,
}

impl<T> Clone for AsyncRx<T> {
    #[inline]
    fn clone(&self) -> Self {
        self.shared.add_rx();
        Self { shared: self.shared.clone() }
    }
}

impl<T> fmt::Debug for AsyncRx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AsyncRx")
    }
}

impl<T> Drop for AsyncRx<T> {
    fn drop(&mut self) {
        self.shared.close_rx();
    }
}

impl<T> AsyncRx<T> {
    #[inline]
    pub(crate) fn new(shared: Arc<ChannelShared<T>>) -> Self {
        Self { shared }
    }

    /// Receive message, will await when channel is empty.
    ///
    /// **NOTE: Do not call `AsyncRx::recv()` concurrently.**
    /// If you need concurrent access, use [MAsyncRx](crate::MAsyncRx) instead.
    ///
    /// Returns `Ok(T)` when successful.
    ///
    /// returns Err([RecvError]) when all Tx dropped.
    #[inline(always)]
    pub async fn recv(&self) -> Result<T, RecvError> {
        return ReceiveFuture { rx: &self, waker: None }.await;
    }

    /// Try to receive message, non-blocking.
    ///
    /// Returns `Ok(T)` on successful.
    ///
    /// Returns Err([TryRecvError::Empty]) when channel is empty.
    ///
    /// Returns Err([TryRecvError::Disconnected]) when all Tx dropped.
    #[inline]
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        if let Some(item) = self.shared.try_recv() {
            self.shared.on_recv();
            return Ok(item);
        } else {
            if self.shared.get_tx_count() == 0 {
                return Err(TryRecvError::Disconnected);
            }
            return Err(TryRecvError::Empty);
        }
    }

    /// Generate a fixed Sized future object that receive a message
    #[inline(always)]
    pub fn make_recv_future<'a>(&'a self) -> ReceiveFuture<'a, T> {
        return ReceiveFuture { rx: &self, waker: None };
    }

    #[inline(always)]
    fn _return_empty(&self) -> TryRecvError {
        if self.shared.get_tx_count() == 0 {
            return TryRecvError::Disconnected;
        }
        return TryRecvError::Empty;
    }

    /// Probe possible messages in the channel (not accurate)
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.shared.len()
    }

    /// Whether there's message in the channel (not accurate)
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.shared.is_empty()
    }

    /// Internal function, might changed in the future.
    ///
    /// Returns `Ok(T)` on successful.
    ///
    /// Return Err([TryRecvError::Empty]) for Poll::Pending case.
    ///
    /// Return Err([TryRecvError::Disconnected]) when all Tx dropped.
    #[inline(always)]
    pub(crate) fn poll_item(
        &self, ctx: &mut Context, o_waker: &mut Option<LockedWaker>,
    ) -> Result<T, TryRecvError> {
        let shared = &self.shared;
        macro_rules! stats {
            ($try: expr, $done: expr) => {
                #[cfg(feature = "profile")]
                {
                    ChannelStats::rx_poll($try);
                    ChannelStats::rx_done();
                }
            };
            ($try: expr) => {
                #[cfg(feature = "profile")]
                {
                    ChannelStats::rx_poll($try);
                }
            };
        }
        // When the result is not TryRecvError::Empty,
        // make sure always take the o_waker out and abandon,
        // to skip the timeout cleaning logic in Drop.
        let backoff = Backoff::new();
        let try_limit: usize = 3;
        for i in 0..try_limit {
            if i > 0 {
                backoff.snooze();
            }
            match shared.try_recv() {
                None => {
                    if i == try_limit - 2 {
                        if shared.reg_recv_async(ctx, o_waker) {
                            stats!(i + 1);
                            // waker is not consumed
                            return Err(self._return_empty());
                        }
                        // NOTE: The other side put something whie reg_send and did not see the waker,
                        // should check the channel again, otherwise might incur a dead lock.
                    } else {
                        // No need to reg again
                    }
                    continue;
                }
                Some(item) => {
                    if let Some(old_waker) = o_waker.take() {
                        old_waker.cancel();
                    }
                    stats!(i + 1, true);
                    shared.on_recv();
                    return Ok(item);
                }
            }
        }
        stats!(try_limit, true);
        return Err(self._return_empty());
    }

    pub fn into_stream(self) -> AsyncStream<T>
    where
        T: Sync + Send + Unpin + 'static,
    {
        AsyncStream::new(self)
    }

    /// Returns count of tx / rx wakers stored in channel for debug purpose
    #[inline]
    #[cfg(test)]
    pub fn get_waker_size(&self) -> (usize, usize) {
        return self.shared.get_waker_size();
    }

    /// Receive a message while **blocking the current thread**. Be careful!
    ///
    /// Returns `Ok(T)` on successful.
    ///
    /// Returns Err([RecvError]) when all Tx dropped.
    ///
    /// **NOTE: Do not use it in async context otherwise will block the runtime.**
    #[inline(always)]
    pub fn recv_blocking(&self) -> Result<T, RecvError> {
        Rx::_recv_blocking(&self.shared)
    }
}

/// A fixed-sized future object constructed by [AsyncRx::make_recv_future()]
pub struct ReceiveFuture<'a, T> {
    rx: &'a AsyncRx<T>,
    waker: Option<LockedWaker>,
}

impl<T> Drop for ReceiveFuture<'_, T> {
    fn drop(&mut self) {
        if let Some(waker) = self.waker.take() {
            // Cancelling the future, poll is not ready
            if waker.abandon() {
                // We are waked, but giving up to recv, should notify another receiver for safety
                self.rx.shared.on_send();
            } else {
                self.rx.shared.clear_recv_wakers(waker.get_seq());
            }
        }
    }
}

impl<T> Future for ReceiveFuture<'_, T> {
    type Output = Result<T, RecvError>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let mut _self = self.get_mut();
        match _self.rx.poll_item(ctx, &mut _self.waker) {
            Err(e) => {
                if !e.is_empty() {
                    let _ = _self.waker.take();
                    return Poll::Ready(Err(RecvError {}));
                } else {
                    return Poll::Pending;
                }
            }
            Ok(item) => {
                debug_assert!(_self.waker.is_none());
                return Poll::Ready(Ok(item));
            }
        }
    }
}

/// For writing generic code with MAsyncRx & AsyncRx
#[async_trait]
pub trait AsyncRxTrait<T: Unpin + Send + 'static>: Send + Sync + 'static {
    /// Receive message, will await when channel is empty.
    ///
    /// Returns `Ok(T)` when successful.
    ///
    /// returns Err([RecvError]) when all Tx dropped.
    async fn recv(&self) -> Result<T, RecvError>;

    /// Try to receive message, non-blocking.
    ///
    /// Returns Ok(T) when successful.
    ///
    /// Returns Err([TryRecvError::Empty]) when channel is empty.
    ///
    /// Returns Err([TryRecvError::Disconnected]) when all Tx dropped.
    fn try_recv(&self) -> Result<T, TryRecvError>;

    /// Generate a fixed Sized future object that receive a message
    fn make_recv_future<'a>(&'a self) -> ReceiveFuture<'a, T>;

    /// Probe possible messages in the channel (not accurate)
    fn len(&self) -> usize;

    /// Whether there's message in the channel (not accurate)
    fn is_empty(&self) -> bool;

    /// Returns count of tx / rx wakers stored in channel for debug purpose
    #[cfg(test)]
    fn get_waker_size(&self) -> (usize, usize);
}

#[async_trait]
impl<T: Unpin + Send + 'static> AsyncRxTrait<T> for AsyncRx<T> {
    #[inline(always)]
    async fn recv(&self) -> Result<T, RecvError> {
        AsyncRx::<T>::recv(self).await
    }

    #[inline(always)]
    fn try_recv(&self) -> Result<T, TryRecvError> {
        AsyncRx::<T>::try_recv(self)
    }

    #[inline(always)]
    fn make_recv_future<'a>(&'a self) -> ReceiveFuture<'a, T> {
        AsyncRx::<T>::make_recv_future(self)
    }

    #[inline(always)]
    fn len(&self) -> usize {
        AsyncRx::len(self)
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
        AsyncRx::<T>::is_empty(self)
    }

    #[inline(always)]
    #[cfg(test)]
    fn get_waker_size(&self) -> (usize, usize) {
        AsyncRx::get_waker_size(self)
    }
}

/// Receiver that works in async context. MC version of [`AsyncRx<T>`] implements [Clone].
///
/// You can use `into()` to convert it to `AsyncRx<T>`.
pub struct MAsyncRx<T>(pub(crate) AsyncRx<T>);

impl<T> Clone for MAsyncRx<T> {
    #[inline]
    fn clone(&self) -> Self {
        let inner = &self.0;
        inner.shared.add_rx();
        Self(AsyncRx { shared: inner.shared.clone() })
    }
}

impl<T> From<MAsyncRx<T>> for AsyncRx<T> {
    fn from(rx: MAsyncRx<T>) -> Self {
        rx.0
    }
}

impl<T> MAsyncRx<T> {
    #[inline]
    pub(crate) fn new(shared: Arc<ChannelShared<T>>) -> Self {
        Self(AsyncRx::new(shared))
    }

    #[inline]
    pub fn into_stream(self) -> AsyncStream<T>
    where
        T: Sync + Send + Unpin + 'static,
    {
        AsyncStream::new(self.0)
    }
}

impl<T> Deref for MAsyncRx<T> {
    type Target = AsyncRx<T>;

    /// inherit all the functions of [AsyncRx]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for MAsyncRx<T> {
    /// inherit all the functions of [AsyncRx]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[async_trait]
impl<T: Unpin + Send + 'static> AsyncRxTrait<T> for MAsyncRx<T> {
    #[inline(always)]
    async fn recv(&self) -> Result<T, RecvError> {
        self.0.recv().await
    }

    #[inline(always)]
    fn try_recv(&self) -> Result<T, TryRecvError> {
        self.0.try_recv()
    }

    /// Generate a fixed Sized future object that receive a message
    #[inline(always)]
    fn make_recv_future<'a>(&'a self) -> ReceiveFuture<'a, T> {
        self.0.make_recv_future()
    }

    /// Probe possible messages in the channel (not accurate)
    #[inline(always)]
    fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there's message in the channel (not accurate)
    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns count of tx / rx wakers stored in channel for debug purpose
    #[inline(always)]
    #[cfg(test)]
    fn get_waker_size(&self) -> (usize, usize) {
        self.0.get_waker_size()
    }
}
