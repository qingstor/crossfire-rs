use crate::blocking_tx::Tx;
use crate::channel::*;
use async_trait::async_trait;
pub use crossbeam::channel::{SendError, TrySendError};
use crossbeam::utils::Backoff;
use std::fmt;
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

/// Sender that works in async context
///
/// **NOTE: this is not cloneable.**
/// If you need concurrent access, use [MAsyncTx](crate::MAsyncTx) instead.
pub struct AsyncTx<T> {
    pub(crate) shared: Arc<ChannelShared<T>>,
}

impl<T> fmt::Debug for AsyncTx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "AsyncTx")
    }
}

impl<T> Drop for AsyncTx<T> {
    fn drop(&mut self) {
        self.shared.close_tx();
    }
}

impl<T: Unpin + Send + 'static> AsyncTx<T> {
    /// Send message. Will await when channel is full.
    ///
    /// **NOTE: Do not call `AsyncTx::send()` concurrently.**
    /// If you need concurrent access, use [MAsyncTx::send()](crate::MAsyncTx) instead.
    ///
    /// Returns `Ok(())` on successful.
    ///
    /// Returns Err([SendError]) when all Rx is dropped.
    #[inline(always)]
    pub async fn send(&self, item: T) -> Result<(), SendError<T>> {
        if self.shared.get_rx_count() == 0 {
            return Err(SendError(item));
        }
        return SendFuture { tx: &self, item: Some(item), waker: None }.await;
    }

    /// Try to send message, non-blocking
    ///
    /// Returns `Ok(())` when successful.
    ///
    /// Returns Err([TrySendError::Full]) on channel full for bounded channel.
    ///
    /// Returns Err([TrySendError::Disconnected]) when all Rx dropped.
    #[inline]
    pub fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        if self.shared.get_rx_count() == 0 {
            return Err(TrySendError::Disconnected(item));
        }
        match self.shared.try_send(item) {
            Err(item) => {
                return Err(TrySendError::Full(item));
            }
            Ok(_) => {
                self.shared.on_send();
                return Ok(());
            }
        }
    }

    /// Generate a fixed Sized future object that send a message
    #[inline(always)]
    pub fn make_send_future<'a>(&'a self, item: T) -> SendFuture<'a, T> {
        return SendFuture { tx: &self, item: Some(item), waker: None };
    }

    #[inline(always)]
    fn _return_full(&self, item: T) -> TrySendError<T> {
        if self.shared.get_rx_count() == 0 {
            return TrySendError::Disconnected(item);
        }
        return TrySendError::Full(item);
    }

    /// Internal function, might changed in the future.
    ///
    /// Returns `Ok(())` on message sent.
    ///
    /// Returns Err([TrySendError::Full]) for Poll::Pending case.
    ///
    /// Returns Err([TrySendError::Disconnected]) when all Rx dropped.
    #[inline(always)]
    pub(crate) fn poll_send<'a>(
        &'a self, ctx: &'a mut Context, mut item: T, o_waker: &'a mut Option<LockedWaker>,
    ) -> Result<(), TrySendError<T>> {
        // When the result is not TrySendError::Full,
        // make sure always take the o_waker out and abandon,
        // to skip the timeout cleaning logic in Drop.
        let backoff = Backoff::new();
        let shared = &self.shared;
        let try_limit: usize = 3;
        macro_rules! stats {
            ($try: expr, $done: expr) => {
                #[cfg(feature = "profile")]
                {
                    ChannelStats::tx_poll($try);
                    ChannelStats::tx_done();
                }
            };
            ($try: expr) => {
                #[cfg(feature = "profile")]
                {
                    ChannelStats::tx_poll($try);
                }
            };
        }
        for i in 0..try_limit {
            if i > 0 {
                backoff.snooze();
            }
            match shared.try_send(item) {
                Err(t) => {
                    if i == try_limit - 2 {
                        if shared.reg_send_async(ctx, o_waker) {
                            stats!(i + 1);
                            // waker is not consumed
                            return Err(self._return_full(t));
                        }
                        // NOTE: The other side put something whie reg_send and did not see the waker,
                        // should check the channel again, otherwise might incur a dead lock.
                    } else {
                        // No need to reg again
                    }
                    item = t;
                    continue;
                }
                Ok(_) => {
                    if let Some(old_waker) = o_waker.take() {
                        old_waker.cancel();
                    }
                    stats!(i + 1, true);
                    shared.on_send();
                    return Ok(());
                }
            }
        }
        stats!(try_limit, true);
        return Err(self._return_full(item));
    }

    /// Send a message while **blocking the current thread**. Be careful!
    ///
    /// Returns `Ok(())`on successful.
    ///
    /// Returns Err([SendError]) when all Rx is dropped.
    ///
    /// **NOTE: Do not use it in async context otherwise will block the runtime.**
    #[inline]
    pub fn send_blocking(&self, item: T) -> Result<(), SendError<T>> {
        Tx::_send_blocking(&self.shared, item)
    }

    /// Just for debugging purpose, to monitor queue size
    #[inline]
    #[cfg(test)]
    pub fn get_waker_size(&self) -> (usize, usize) {
        return self.shared.get_waker_size();
    }
}

impl<T> AsyncTx<T> {
    #[inline]
    pub(crate) fn new(shared: Arc<ChannelShared<T>>) -> Self {
        Self { shared }
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
}

/// A fixed-sized future object constructed by [AsyncTx::make_send_future()]
pub struct SendFuture<'a, T: Unpin> {
    tx: &'a AsyncTx<T>,
    item: Option<T>,
    waker: Option<LockedWaker>,
}

impl<T: Unpin> Drop for SendFuture<'_, T> {
    fn drop(&mut self) {
        if let Some(waker) = self.waker.take() {
            // Cancelling the future, poll is not ready
            if waker.abandon() {
                // We are waked, but give up sending, should notify another sender for safety
                self.tx.shared.on_recv();
            } else {
                self.tx.shared.clear_send_wakers(waker.get_seq());
            }
        }
    }
}

impl<T: Unpin + Send + 'static> Future for SendFuture<'_, T> {
    type Output = Result<(), SendError<T>>;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let mut _self = self.get_mut();
        let item = _self.item.take().unwrap();
        let tx = _self.tx;
        let r = tx.poll_send(ctx, item, &mut _self.waker);
        match r {
            Ok(()) => {
                return Poll::Ready(Ok(()));
            }
            Err(TrySendError::Disconnected(t)) => {
                return Poll::Ready(Err(SendError(t)));
            }
            Err(TrySendError::Full(t)) => {
                _self.item.replace(t);
                return Poll::Pending;
            }
        }
    }
}

/// For writing generic code with MAsyncTx & AsyncTx
#[async_trait]
pub trait AsyncTxTrait<T: Unpin + Send + 'static>: Send + Sync + 'static {
    /// Send message. Will await when channel is full.
    ///
    /// Returns `Ok(())` on successful.
    ///
    /// Returns Err([SendError]) when all Rx is dropped.
    async fn send(&self, item: T) -> Result<(), SendError<T>>;

    /// Just for debugging purpose, to monitor queue size
    #[cfg(test)]
    fn get_waker_size(&self) -> (usize, usize);

    /// Try to send message, non-blocking
    ///
    /// Returns `Ok(())` when successful.
    ///
    /// Returns Err([TrySendError::Full]) on channel full for bounded channel.
    ///
    /// Returns Err([TrySendError::Disconnected]) when all Rx dropped.
    fn try_send(&self, item: T) -> Result<(), TrySendError<T>>;

    /// Probe possible messages in the channel (not accurate)
    fn len(&self) -> usize;

    /// Whether there's message in the channel (not accurate)
    fn is_empty(&self) -> bool;

    /// Generate a fixed Sized future object that send a message
    fn make_send_future<'a>(&'a self, item: T) -> SendFuture<'a, T>;
}

#[async_trait]
impl<T: Unpin + Send + 'static> AsyncTxTrait<T> for AsyncTx<T> {
    #[inline(always)]
    async fn send(&self, item: T) -> Result<(), SendError<T>> {
        AsyncTx::send(self, item).await
    }

    #[inline(always)]
    #[cfg(test)]
    fn get_waker_size(&self) -> (usize, usize) {
        AsyncTx::get_waker_size(self)
    }

    #[inline(always)]
    fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        AsyncTx::try_send(self, item)
    }

    #[inline(always)]
    fn len(&self) -> usize {
        AsyncTx::len(self)
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
        AsyncTx::is_empty(self)
    }

    #[inline(always)]
    fn make_send_future<'a>(&'a self, item: T) -> SendFuture<'a, T> {
        AsyncTx::make_send_future(self, item)
    }
}

/// Sender that works in async context, MP version of [`AsyncTx<T>`] implements [Clone].
///
/// You can use `into()` to convert it to `AsyncTx<T>`.
pub struct MAsyncTx<T>(pub(crate) AsyncTx<T>);

impl<T: Unpin> Clone for MAsyncTx<T> {
    #[inline]
    fn clone(&self) -> Self {
        let inner = &self.0;
        inner.shared.add_tx();
        Self(AsyncTx::new(inner.shared.clone()))
    }
}

impl<T> MAsyncTx<T> {
    #[inline]
    pub(crate) fn new(shared: Arc<ChannelShared<T>>) -> Self {
        Self(AsyncTx::new(shared))
    }
}

impl<T> Deref for MAsyncTx<T> {
    type Target = AsyncTx<T>;

    /// inherit all the functions of [AsyncTx]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for MAsyncTx<T> {
    /// inherit all the functions of [AsyncTx]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[async_trait]
impl<T: Unpin + Send + 'static> AsyncTxTrait<T> for MAsyncTx<T> {
    #[inline(always)]
    async fn send(&self, item: T) -> Result<(), SendError<T>> {
        self.0.send(item).await
    }

    #[inline(always)]
    fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        self.0.try_send(item)
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

    fn make_send_future<'a>(&'a self, item: T) -> SendFuture<'a, T> {
        self.0.make_send_future(item)
    }

    /// Just for debugging purpose, to monitor queue size
    #[inline(always)]
    #[cfg(test)]
    fn get_waker_size(&self) -> (usize, usize) {
        self.0.get_waker_size()
    }
}
