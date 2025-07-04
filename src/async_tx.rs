use crate::channel::*;
use async_trait::async_trait;
use crossbeam::channel::Sender;
pub use crossbeam::channel::{SendError, TrySendError};
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
    pub(crate) sender: Sender<T>,
    pub(crate) shared: Arc<ChannelShared>,
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
        match self.try_send(item) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Full(t)) => {
                return SendFuture { tx: &self, item: Some(t), waker: None }.await;
            }
            Err(TrySendError::Disconnected(t)) => return Err(SendError(t)),
        }
    }

    /// Generate a fixed Sized future object that send a message
    #[inline(always)]
    pub fn make_send_future<'a>(&'a self, item: T) -> SendFuture<'a, T> {
        return SendFuture { tx: &self, item: Some(item), waker: None };
    }

    /// This is only useful when you're writing your own future.
    ///
    /// Returns `Ok(())` on message sent.
    ///
    /// Returns Err([TrySendError::Full]) for Poll::Pending case.
    ///
    /// Returns Err([TrySendError::Disconnected]) when all Rx dropped.
    #[inline(always)]
    pub fn poll_send<'a>(
        &'a self, ctx: &'a mut Context, mut item: T, o_waker: &'a mut Option<LockedWaker>,
    ) -> Result<(), TrySendError<T>> {
        // When the result is not TrySendError::Full,
        // make sure always take the o_waker out and abandon,
        // to skip the timeout cleaning logic in Drop.
        let r = self.try_send(item);
        if let Err(TrySendError::Full(t)) = r {
            if self.shared.get_rx_count() == 0 {
                // Check channel close before sleep
                return Err(TrySendError::Disconnected(t));
            }
            if let Some(old_waker) = o_waker.as_ref() {
                if old_waker.is_waked() {
                    let _ = o_waker.take(); // reg again
                } else {
                    // False wakeup
                    return Err(TrySendError::Full(t));
                }
            }
            item = t;
        } else {
            if let Some(old_waker) = o_waker.take() {
                self.shared.cancel_send_waker(old_waker);
            }
            return r;
        }
        let waker = self.shared.reg_send(ctx);
        // NOTE: The other side put something whie reg_send and did not see the waker,
        // should check the channel again, otherwise might incur a dead lock.
        let r = self.try_send(item);
        if let Err(TrySendError::Full(t)) = r {
            if self.shared.get_rx_count() == 0 {
                // Check channel close before sleep, otherwise might block forever
                // Confirmed by test_pressure_1_tx_blocking_1_rx_async()
                return Err(TrySendError::Disconnected(t));
            }
            o_waker.replace(waker);
            return Err(TrySendError::Full(t));
        } else {
            // Ok or Disconnected
            self.shared.cancel_send_waker(waker);
        }
        return r;
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
    pub(crate) fn new(sender: Sender<T>, shared: Arc<ChannelShared>) -> Self {
        Self { sender, shared }
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
        match self.sender.try_send(item) {
            Err(e) => return Err(e),
            Ok(_) => {
                self.shared.on_send();
                return Ok(());
            }
        }
    }

    /// Probe possible messages in the channel (not accurate)
    #[inline]
    pub fn len(&self) -> usize {
        self.sender.len()
    }

    /// Whether there's message in the channel (not accurate)
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.sender.is_empty()
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
        match self.sender.send(item) {
            Ok(()) => {
                self.shared.on_send();
                return Ok(());
            }
            Err(e) => return Err(e),
        }
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
        Self(AsyncTx::new(inner.sender.clone(), inner.shared.clone()))
    }
}

impl<T> MAsyncTx<T> {
    #[inline]
    pub(crate) fn new(send: Sender<T>, shared: Arc<ChannelShared>) -> Self {
        Self(AsyncTx::new(send, shared))
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
