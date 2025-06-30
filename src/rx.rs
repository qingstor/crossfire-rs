use crate::channel::*;
use crate::m_rx::*;
use crate::stream::AsyncStream;
use async_trait::async_trait;
use crossbeam::channel::Receiver;
pub use crossbeam::channel::{RecvError, RecvTimeoutError, TryRecvError};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

/// Receiver that works in blocking context
pub struct Rx<T> {
    pub(crate) recv: Receiver<T>,
    pub(crate) shared: Arc<ChannelShared>,
}

impl<T> fmt::Debug for Rx<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Rx")
    }
}

impl<T> Drop for Rx<T> {
    fn drop(&mut self) {
        self.shared.close_rx();
    }
}

impl<T> Rx<T> {
    #[inline]
    pub(crate) fn new(recv: Receiver<T>, shared: Arc<ChannelShared>) -> Self {
        Self { recv, shared }
    }

    /// Receive message, will block when channel is empty.
    ///
    /// Returns Ok(T) when successful.
    ///
    /// Returns Err([RecvError]) when all Tx dropped.
    #[inline]
    pub fn recv<'a>(&'a self) -> Result<T, RecvError> {
        match self.recv.recv() {
            Err(e) => return Err(e),
            Ok(i) => {
                self.shared.on_recv();
                return Ok(i);
            }
        }
    }

    /// Try to receive message, non-blocking.
    ///
    /// Returns Ok(T) when successful.
    ///
    /// Returns Err([TryRecvError::Empty]) when channel is empty.
    ///
    /// returns Err([TryRecvError::Disconnected]) when all Tx dropped.
    #[inline]
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        match self.recv.try_recv() {
            Err(e) => return Err(e),
            Ok(i) => {
                self.shared.on_recv();
                return Ok(i);
            }
        }
    }

    /// Waits for a message to be received from the channel, but only for a limited time.
    /// Will block when channel is empty.
    ///
    /// Returns Ok(T) when successful.
    ///
    /// Returns Err([RecvTimeoutError::Timeout]) when a message could not be received because the channel is empty and the operation timed out.
    ///
    /// returns Err([RecvTimeoutError::Disconnected]) when all Tx dropped.
    #[inline]
    pub fn recv_timeout(&self, timeout: Duration) -> Result<T, RecvTimeoutError> {
        match self.recv.recv_timeout(timeout) {
            Err(e) => return Err(e),
            Ok(i) => {
                self.shared.on_recv();
                return Ok(i);
            }
        }
    }

    /// Probe possible messages in the channel (not accurate)
    #[inline]
    pub fn len(&self) -> usize {
        self.recv.len()
    }

    /// Whether there's message in the channel (not accurate)
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.recv.is_empty()
    }
}

/// Receiver that works in async context
///
/// **NOTE: this is not cloneable.**
/// If you need concurrent access, use [MAsyncRx](crate::MAsyncRx) instead.
pub struct AsyncRx<T> {
    pub(crate) recv: Receiver<T>,
    pub(crate) shared: Arc<ChannelShared>,
}

impl<T> Clone for AsyncRx<T> {
    #[inline]
    fn clone(&self) -> Self {
        self.shared.add_rx();
        Self { recv: self.recv.clone(), shared: self.shared.clone() }
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
    pub(crate) fn new(recv: Receiver<T>, shared: Arc<ChannelShared>) -> Self {
        Self { recv, shared }
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
        match self.try_recv() {
            Err(TryRecvError::Disconnected) => {
                return Err(RecvError {});
            }
            Ok(item) => return Ok(item),
            _ => {
                return ReceiveFuture { rx: &self, waker: None }.await;
            }
        }
    }

    /// Try to receive message, non-blocking.
    ///
    /// Returns `Ok(T)` on successful.
    ///
    /// Returns Err([TryRecvError::Empty]) when channel is empty.
    ///
    /// Returns Err([TryRecvError::Disconnected]) when all Tx dropped.
    #[inline(always)]
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        match self.recv.try_recv() {
            Err(e) => return Err(e),
            Ok(i) => {
                self.shared.on_recv();
                return Ok(i);
            }
        }
    }

    /// Generate a fixed Sized future object that receive a message
    #[inline(always)]
    pub fn make_recv_future<'a>(&'a self) -> ReceiveFuture<'a, T> {
        return ReceiveFuture { rx: &self, waker: None };
    }

    /// Probe possible messages in the channel (not accurate)
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.recv.len()
    }

    /// Whether there's message in the channel (not accurate)
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.recv.is_empty()
    }

    /// This is only useful when you're writing your own future.
    ///
    /// Returns `Ok(T)` on successful.
    ///
    /// Return Err([TryRecvError::Empty]) for Poll::Pending case.
    ///
    /// Return Err([TryRecvError::Disconnected]) when all Tx dropped.
    #[inline(always)]
    pub fn poll_item(
        &self, ctx: &mut Context, o_waker: &mut Option<LockedWaker>,
    ) -> Result<T, TryRecvError> {
        // When the result is not TryRecvError::Empty,
        // make sure always take the o_waker out and abandon,
        // to skip the timeout cleaning logic in Drop.
        let r = self.try_recv();
        if let Err(TryRecvError::Empty) = &r {
            if let Some(old_waker) = o_waker.as_ref() {
                if old_waker.is_waked() {
                    let _ = o_waker.take(); // should reg again
                } else {
                    if self.shared.get_tx_count() == 0 {
                        // Check channel close before sleep
                        return Err(TryRecvError::Disconnected);
                    }
                    // False wake up, sleep again
                    return Err(TryRecvError::Empty);
                }
            }
        } else {
            if let Some(old_waker) = o_waker.take() {
                self.shared.cancel_recv_waker(old_waker);
            }
            return r;
        }
        let waker = self.shared.reg_recv(ctx);
        // NOTE: The other side put something whie reg_send and did not see the waker,
        // should check the channel again, otherwise might incur a dead lock.
        let r = self.try_recv();
        if let Err(TryRecvError::Empty) = &r {
            if self.shared.get_tx_count() == 0 {
                // Check channel close before sleep, otherwise might block forever
                // Confirmed by test_pressure_1_tx_blocking_1_rx_async()
                return Err(TryRecvError::Disconnected);
            }
            o_waker.replace(waker);
        } else {
            self.shared.cancel_recv_waker(waker);
        }
        return r;
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
        match self.recv.recv() {
            Err(e) => return Err(e),
            Ok(i) => {
                self.shared.on_recv();
                return Ok(i);
            }
        }
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
                    return Poll::Ready(Err(RecvError {}));
                } else {
                    return Poll::Pending;
                }
            }
            Ok(item) => {
                return Poll::Ready(Ok(item));
            }
        }
    }
}

impl<T> From<MRx<T>> for Rx<T> {
    fn from(rx: MRx<T>) -> Self {
        rx.0
    }
}

impl<T> From<MAsyncRx<T>> for AsyncRx<T> {
    fn from(rx: MAsyncRx<T>) -> Self {
        rx.0
    }
}

/// For writing generic code with MRx & Rx
pub trait BlockingRxTrait<T: Send + 'static>: Send + 'static {
    /// Receive message, will block when channel is empty.
    ///
    /// Returns `Ok(T)` when successful.
    ///
    /// Returns Err([RecvError]) when all Tx dropped.
    fn recv<'a>(&'a self) -> Result<T, RecvError>;

    /// Try to receive message, non-blocking.
    ///
    /// Returns `Ok(T)` when successful.
    ///
    /// Returns Err([TryRecvError::Empty]) when channel is empty.
    ///
    /// Returns Err([TryRecvError::Disconnected]) when all Tx dropped.
    fn try_recv(&self) -> Result<T, TryRecvError>;

    /// Waits for a message to be received from the channel, but only for a limited time.
    /// Will block when channel is empty.
    ///
    /// Returns Ok(T) when successful.
    ///
    /// Returns Err([RecvTimeoutError::Timeout]) when a message could not be received because the channel is empty and the operation timed out.
    ///
    /// returns Err([RecvTimeoutError::Disconnected]) when all Tx dropped.
    fn recv_timeout(&self, timeout: Duration) -> Result<T, RecvTimeoutError>;

    /// Probe possible messages in the channel (not accurate)
    fn len(&self) -> usize;

    /// Whether there's message in the channel (not accurate)
    fn is_empty(&self) -> bool;
}

impl<T: Send + 'static> BlockingRxTrait<T> for Rx<T> {
    #[inline(always)]
    fn recv<'a>(&'a self) -> Result<T, RecvError> {
        Rx::recv(self)
    }

    #[inline(always)]
    fn try_recv(&self) -> Result<T, TryRecvError> {
        Rx::try_recv(self)
    }

    #[inline(always)]
    fn recv_timeout(&self, timeout: Duration) -> Result<T, RecvTimeoutError> {
        Rx::recv_timeout(self, timeout)
    }

    #[inline(always)]
    fn len(&self) -> usize {
        Rx::len(self)
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
        Rx::is_empty(self)
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
        AsyncRx::recv(self).await
    }

    #[inline(always)]
    fn try_recv(&self) -> Result<T, TryRecvError> {
        AsyncRx::try_recv(self)
    }

    #[inline(always)]
    fn make_recv_future<'a>(&'a self) -> ReceiveFuture<'a, T> {
        AsyncRx::make_recv_future(self)
    }

    #[inline(always)]
    fn len(&self) -> usize {
        AsyncRx::len(self)
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
        AsyncRx::is_empty(self)
    }

    #[inline(always)]
    #[cfg(test)]
    fn get_waker_size(&self) -> (usize, usize) {
        AsyncRx::get_waker_size(self)
    }
}
