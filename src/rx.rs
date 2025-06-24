use crate::channel::*;
use crate::m_rx::*;
use crate::stream::AsyncStream;
use async_trait::async_trait;
use crossbeam::channel::Receiver;
pub use crossbeam::channel::{RecvError, TryRecvError};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

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

    /// Return a crossbeam Receiver, you should make sure to call on_recv() after receiving a
    /// message
    /// (If you know what you're doing)
    #[inline(always)]
    pub fn raw(&self) -> &Receiver<T> {
        &self.recv
    }
}

/// Receiver that works in async context
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

    #[inline]
    pub async fn recv(&self) -> Result<T, RecvError> {
        match self.try_recv() {
            Err(e) => {
                if e.is_empty() {
                    return ReceiveFuture { rx: &self, waker: None }.await;
                }
                return Err(RecvError {});
            }
            Ok(item) => return Ok(item),
        }
    }

    /// Receive a message while blocking the current thread. (If you know what you're doing)
    #[inline]
    pub fn recv_blocking(&self) -> Result<T, RecvError> {
        match self.recv.recv() {
            Err(e) => return Err(e),
            Ok(i) => {
                self.shared.on_recv();
                return Ok(i);
            }
        }
    }

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

    /// Generate a fixed Sized future object that receive a message
    #[inline]
    pub fn make_recv_future<'a>(&'a self) -> ReceiveFuture<'a, T> {
        return ReceiveFuture { rx: &self, waker: None };
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

    /// This is only useful when you're writing your own future
    #[inline(always)]
    pub fn poll_item(
        &self, ctx: &mut Context, waker: &mut Option<LockedWaker>,
    ) -> Result<T, TryRecvError> {
        match self.recv.try_recv() {
            Err(e) => {
                if e.is_empty() {
                    if let Some(old_waker) = waker.as_ref() {
                        if old_waker.is_waked() {
                            let _ = waker.take(); // should reg again
                        } else {
                            // False wake up
                            if self.shared.get_tx_count() == 0 {
                                // Check channel close before sleep
                                return Err(TryRecvError::Disconnected);
                            }
                            return Err(e);
                        }
                    }
                } else {
                    if let Some(old_waker) = waker.take() {
                        old_waker.abandon();
                    }
                    return Err(e);
                }
            }
            Ok(item) => {
                self.shared.on_recv();
                if let Some(old_waker) = waker.take() {
                    old_waker.abandon();
                }
                return Ok(item);
            }
        }
        let _waker = self.shared.reg_recv(ctx);
        match self.recv.try_recv() {
            Err(TryRecvError::Empty) => {
                _waker.commit();
                waker.replace(_waker);
                if self.shared.get_tx_count() == 0 {
                    // Check channel close before sleep, otherwise might block forever
                    // Confirmed by test_presure_1_tx_blocking_1_rx_async()
                    return Err(TryRecvError::Disconnected);
                }
                return Err(TryRecvError::Empty);
            }
            Err(TryRecvError::Disconnected) => {
                _waker.cancel();
                return Err(TryRecvError::Disconnected);
            }
            Ok(item) => {
                if !_waker.is_waked() {
                    _waker.cancel();
                }
                self.shared.on_recv();
                return Ok(item);
            }
        }
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
}

pub struct ReceiveFuture<'a, T> {
    rx: &'a AsyncRx<T>,
    waker: Option<LockedWaker>,
}

impl<T> Drop for ReceiveFuture<'_, T> {
    fn drop(&mut self) {
        if let Some(waker) = self.waker.take() {
            if waker.abandon() {
                // We are waked, but abandoning, should notify another receiver
                if !self.rx.recv.is_empty() {
                    self.rx.shared.on_send();
                }
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
    fn recv<'a>(&'a self) -> Result<T, RecvError>;

    fn try_recv(&self) -> Result<T, TryRecvError>;

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

    /// Probe possible messages in the channel (not accurate)
    #[inline(always)]
    fn len(&self) -> usize {
        Rx::len(self)
    }

    /// Whether there's message in the channel (not accurate)
    #[inline(always)]
    fn is_empty(&self) -> bool {
        Rx::is_empty(self)
    }
}

/// For writing generic code with MAsyncRx & AsyncRx
#[async_trait]
pub trait AsyncRxTrait<T: Unpin + Send + 'static>: Send + Sync + 'static {
    async fn recv(&self) -> Result<T, RecvError>;

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

    /// Generate a fixed Sized future object that receive a message
    #[inline(always)]
    fn make_recv_future<'a>(&'a self) -> ReceiveFuture<'a, T> {
        AsyncRx::make_recv_future(self)
    }

    /// Probe possible messages in the channel (not accurate)
    #[inline(always)]
    fn len(&self) -> usize {
        AsyncRx::len(self)
    }

    /// Whether there's message in the channel (not accurate)
    #[inline(always)]
    fn is_empty(&self) -> bool {
        AsyncRx::is_empty(self)
    }

    /// Returns count of tx / rx wakers stored in channel for debug purpose
    #[inline(always)]
    #[cfg(test)]
    fn get_waker_size(&self) -> (usize, usize) {
        AsyncRx::get_waker_size(self)
    }
}
