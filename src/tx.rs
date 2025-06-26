use crate::channel::*;
use async_trait::async_trait;
pub use crossbeam::channel::{SendError, Sender, TrySendError};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

/// Sender that works in blocking context
pub struct Tx<T> {
    pub(crate) sender: Sender<T>,
    pub(crate) shared: Arc<ChannelShared>,
}

impl<T> fmt::Debug for Tx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Tx")
    }
}

impl<T> Drop for Tx<T> {
    fn drop(&mut self) {
        self.shared.close_tx();
    }
}

impl<T> Tx<T> {
    #[inline]
    pub(crate) fn new(sender: Sender<T>, shared: Arc<ChannelShared>) -> Self {
        Self { sender, shared }
    }

    #[inline]
    pub fn send(&self, item: T) -> Result<(), SendError<T>> {
        match self.sender.send(item) {
            Err(e) => return Err(e),
            Ok(_) => {
                self.shared.on_send();
                return Ok(());
            }
        }
    }

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
}

/// Sender that works in async context
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

impl<T> AsyncTx<T> {
    #[inline]
    pub(crate) fn new(sender: Sender<T>, shared: Arc<ChannelShared>) -> Self {
        Self { sender, shared }
    }

    /// Send a message while blocking the current thread. (Used outside async context,
    /// if you know what you're doing)
    #[inline]
    pub fn send_blocking(&self, item: T) -> Result<(), SendError<T>> {
        self.sender.send(item)
    }

    /// Try to sed message without waiting
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
}

impl<T: Unpin + Send + 'static> AsyncTx<T> {
    /// Sed message
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

    /// for your own future impl
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
                old_waker.abandon();
            }
            return r;
        }
        let waker = self.shared.reg_send(ctx);
        // NOTE: The other side put something whie reg_send and did not see the waker,
        // should check the channel again, otherwise might incur a dead lock.
        match self.sender.try_send(item) {
            Ok(()) => {
                self.shared.on_send();
                waker.abandon();
                return Ok(());
            }
            Err(TrySendError::Full(t)) => {
                o_waker.replace(waker);
                if self.shared.get_rx_count() == 0 {
                    // Check channel close before sleep, otherwise might block forever
                    // Confirmed by test_presure_1_tx_blocking_1_rx_async()
                    return Err(TrySendError::Disconnected(t));
                }
                return Err(TrySendError::Full(t));
            }
            Err(TrySendError::Disconnected(t)) => {
                waker.abandon();
                return Err(TrySendError::Disconnected(t));
            }
        }
    }

    /// Just for debugging purpose, to monitor queue size
    #[inline]
    #[cfg(test)]
    pub fn get_waker_size(&self) -> (usize, usize) {
        return self.shared.get_waker_size();
    }
}

/// A future of send for AsyncTx
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
                // We are waked, but give up sending, should notify another sender
                if !self.tx.sender.is_full() {
                    self.tx.shared.on_recv();
                }
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

/// For writing generic code with MTx & Tx
pub trait BlockingTxTrait<T: Send + 'static>: Send + 'static {
    fn send(&self, _item: T) -> Result<(), SendError<T>>;

    fn try_send(&self, _item: T) -> Result<(), TrySendError<T>>;

    fn len(&self) -> usize;

    fn is_empty(&self) -> bool;
}

impl<T: Send + 'static> BlockingTxTrait<T> for Tx<T> {
    #[inline(always)]
    fn send(&self, item: T) -> Result<(), SendError<T>> {
        Tx::send(self, item)
    }

    #[inline(always)]
    fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        Tx::try_send(self, item)
    }

    #[inline(always)]
    fn len(&self) -> usize {
        Tx::len(self)
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
        Tx::is_empty(self)
    }
}

/// For writing generic code with MAsyncTx & AsyncTx
#[async_trait]
pub trait AsyncTxTrait<T: Unpin + Send + 'static>: Send + Sync + 'static {
    /// Sed message
    async fn send(&self, item: T) -> Result<(), SendError<T>>;

    /// Just for debugging purpose, to monitor queue size
    #[cfg(test)]
    fn get_waker_size(&self) -> (usize, usize);

    /// Try to sed message without waiting
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

    /// Just for debugging purpose, to monitor queue size
    #[inline(always)]
    #[cfg(test)]
    fn get_waker_size(&self) -> (usize, usize) {
        AsyncTx::get_waker_size(self)
    }

    #[inline(always)]
    fn try_send(&self, item: T) -> Result<(), TrySendError<T>> {
        AsyncTx::try_send(self, item)
    }

    /// Probe possible messages in the channel (not accurate)
    #[inline(always)]
    fn len(&self) -> usize {
        AsyncTx::len(self)
    }

    /// Whether there's message in the channel (not accurate)
    #[inline(always)]
    fn is_empty(&self) -> bool {
        AsyncTx::is_empty(self)
    }

    /// Generate a fixed Sized future object that send a message
    #[inline(always)]
    fn make_send_future<'a>(&'a self, item: T) -> SendFuture<'a, T> {
        AsyncTx::make_send_future(self, item)
    }
}
